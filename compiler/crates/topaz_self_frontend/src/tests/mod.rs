//! Embedded-front-end tests grouped by source, preview, stage, and product edge.
//! Common package fixtures and exchange helpers live here so the split test
//! families exercise one compiler source inventory.

use super::*;
use std::cell::Cell;
use topaz_diag::FileId;
use topaz_lexer::{LayoutOptions, lex, normalize_with_options};
use topaz_syntax::LangVersion;
use topaz_value::JsonValue;

const RESOLVER_ENTRY_SOURCE: &str = "import lib { value }\nlet answer = value\n";
const RESOLVER_LIBRARY_SOURCE: &str = concat!(
    "protocol Measure { function measure(value: Self) -> int }\n",
    "export const value = 42\n",
);
const NESTED_MISSING_ENTRY_SOURCE: &str = "import missing.lib\nlet answer = 42\n";
const LONE_CR_COMMENTED_IMPORT_SOURCE: &str = "// comment\rimport lib { value }\nlet answer = 42\n";
const SEPARATED_IMPORT_HEAD_SOURCES: [(&str, &str); 3] = [
    ("physical-newline", "import\nlib\n"),
    ("semicolon", "import;lib\n"),
    ("line-comment-newline", "import // comment\nlib\n"),
];
const DOTTED_IMPORT_TRIVIA_SOURCES: [(&str, &str); 5] = [
    ("spaces", "import lib . sub { value }\nlet answer = value\n"),
    (
        "block-comment",
        "import lib/* comment */.sub { value }\nlet answer = value\n",
    ),
    (
        "line-comment-before-dot",
        "import lib // comment\n  .sub { value }\nlet answer = value\n",
    ),
    (
        "newline-before-dot",
        "import lib\n  .sub { value }\nlet answer = value\n",
    ),
    (
        "newline-after-dot",
        "import lib.\n  sub { value }\nlet answer = value\n",
    ),
];
const NON_IDENTIFIER_IMPORT_PATH_SOURCES: [(&str, &str); 4] = [
    ("keyword-first-segment", "import function\n"),
    ("underscore-first-segment", "import _\n"),
    ("keyword-dotted-segment", "import lib.function\n"),
    ("tagged-string-dotted-segment", "import lib.sub\"text\"\n"),
];
const COMMENT_BRACED_IMPORT_PROLOGUE_SOURCES: [(&str, &str); 2] = [
    (
        "line-comment-open-brace",
        "import lib // {\nimport other\nlet answer = 42\n",
    ),
    (
        "block-comment-open-brace",
        "import lib /* { */\nimport other\nlet answer = 42\n",
    ),
];
const UNDECLARED_EXTERN_ENTRY_SOURCE: &str = "import host.video { resizePng }\nlet answer = 42\n";
const EXTERN_REPLAY_ENTRY_SOURCE: &str = "import host.math { add }\nlet answer = add(20, 22)\n";
const EXTERN_REPLAY_MODULE_SOURCE: &str =
    "export function add(left: int, right: int) -> int { left + right }\n";
const RESERVED_TOPAZ_ENTRY_SOURCE: &str = "import topaz.internal\nlet answer = 42\n";

struct ResolverFixtureHost {
    responses: Cell<u32>,
}

impl ResolverFixtureHost {
    fn new() -> Self {
        Self {
            responses: Cell::new(0),
        }
    }
}

impl topaz_kernel::HostFactSource for ResolverFixtureHost {
    fn respond(
        &self,
        _request: &topaz_kernel::KernelRequest,
        query: &topaz_kernel::HostQuery,
    ) -> topaz_kernel::HostFact {
        self.responses.set(self.responses.get() + 1);
        match query {
            topaz_kernel::HostQuery::ReadSource { logical_path, .. } => {
                topaz_kernel::HostFact::Source(match logical_path.as_str() {
                    "root/main.tpz" => {
                        topaz_kernel::SourceFact::Present(RESOLVER_ENTRY_SOURCE.to_string())
                    }
                    "root/lib.tpz" => {
                        topaz_kernel::SourceFact::Present(RESOLVER_LIBRARY_SOURCE.to_string())
                    }
                    _ => topaz_kernel::SourceFact::Missing,
                })
            }
            topaz_kernel::HostQuery::ListDirectory { logical_path, .. } => {
                topaz_kernel::HostFact::Directory(if logical_path == "root" {
                    topaz_kernel::DirectoryFact::Present(vec![
                        topaz_kernel::DirectoryEntry {
                            name: "lib.tpz".to_string(),
                            kind: topaz_kernel::DirectoryEntryKind::File,
                        },
                        topaz_kernel::DirectoryEntry {
                            name: "main.tpz".to_string(),
                            kind: topaz_kernel::DirectoryEntryKind::File,
                        },
                    ])
                } else {
                    topaz_kernel::DirectoryFact::Missing
                })
            }
            topaz_kernel::HostQuery::PhysicalContainment { logical_path, .. } => {
                topaz_kernel::HostFact::Containment(topaz_kernel::ContainmentFact::Inside {
                    alias_class: format!("fixture:{logical_path}"),
                })
            }
        }
    }
}

struct NoFactHost;

impl topaz_kernel::HostFactSource for NoFactHost {
    fn respond(
        &self,
        _request: &topaz_kernel::KernelRequest,
        query: &topaz_kernel::HostQuery,
    ) -> topaz_kernel::HostFact {
        panic!("replay requested hidden host fact: {query:?}")
    }
}

struct DottedEntryResolverHost;

impl topaz_kernel::HostFactSource for DottedEntryResolverHost {
    fn respond(
        &self,
        _request: &topaz_kernel::KernelRequest,
        query: &topaz_kernel::HostQuery,
    ) -> topaz_kernel::HostFact {
        match query {
            topaz_kernel::HostQuery::ReadSource { logical_path, .. } => {
                topaz_kernel::HostFact::Source(
                    if matches!(
                        logical_path.as_str(),
                        "root/main..tpz" | "root..dir/main.tpz" | "root/../main.tpz"
                    ) {
                        topaz_kernel::SourceFact::Present("let answer = 42\n".to_string())
                    } else {
                        topaz_kernel::SourceFact::Missing
                    },
                )
            }
            topaz_kernel::HostQuery::ListDirectory { logical_path, .. } => {
                topaz_kernel::HostFact::Directory(
                    if matches!(logical_path.as_str(), "root" | "root..dir") {
                        topaz_kernel::DirectoryFact::Present(vec![topaz_kernel::DirectoryEntry {
                            name: "main..tpz".to_string(),
                            kind: topaz_kernel::DirectoryEntryKind::File,
                        }])
                    } else {
                        topaz_kernel::DirectoryFact::Missing
                    },
                )
            }
            topaz_kernel::HostQuery::PhysicalContainment { logical_path, .. } => {
                assert!(
                    matches!(
                        logical_path.as_str(),
                        "root/main..tpz" | "root..dir/main.tpz"
                    ),
                    "self-host resolver requested unused root containment for {logical_path}"
                );
                topaz_kernel::HostFact::Containment(topaz_kernel::ContainmentFact::Inside {
                    alias_class: format!("dotted-entry:{logical_path}"),
                })
            }
        }
    }
}

struct OutsideEntryResolverHost;

impl topaz_kernel::HostFactSource for OutsideEntryResolverHost {
    fn respond(
        &self,
        _request: &topaz_kernel::KernelRequest,
        query: &topaz_kernel::HostQuery,
    ) -> topaz_kernel::HostFact {
        match query {
            topaz_kernel::HostQuery::ReadSource { logical_path, .. }
                if logical_path == "root/main.tpz" =>
            {
                topaz_kernel::HostFact::Source(topaz_kernel::SourceFact::Present(
                    RESOLVER_ENTRY_SOURCE.to_string(),
                ))
            }
            topaz_kernel::HostQuery::PhysicalContainment { logical_path, .. }
                if logical_path == "root/main.tpz" =>
            {
                topaz_kernel::HostFact::Containment(topaz_kernel::ContainmentFact::Outside)
            }
            _ => panic!("outside entry requested an imported-module fact: {query:?}"),
        }
    }
}

struct EntryOnlyResolverHost {
    source: &'static str,
    alias_class: &'static str,
}

impl topaz_kernel::HostFactSource for EntryOnlyResolverHost {
    fn respond(
        &self,
        _request: &topaz_kernel::KernelRequest,
        query: &topaz_kernel::HostQuery,
    ) -> topaz_kernel::HostFact {
        match query {
            topaz_kernel::HostQuery::ReadSource { logical_path, .. }
                if logical_path == "root/main.tpz" =>
            {
                topaz_kernel::HostFact::Source(topaz_kernel::SourceFact::Present(
                    self.source.to_string(),
                ))
            }
            topaz_kernel::HostQuery::PhysicalContainment { logical_path, .. }
                if logical_path == "root/main.tpz" =>
            {
                topaz_kernel::HostFact::Containment(topaz_kernel::ContainmentFact::Inside {
                    alias_class: format!("{}:entry", self.alias_class),
                })
            }
            _ => panic!("non-import source requested an imported-module fact: {query:?}"),
        }
    }
}

struct DottedImportTriviaResolverHost {
    source: &'static str,
    alias_class: &'static str,
}

impl topaz_kernel::HostFactSource for DottedImportTriviaResolverHost {
    fn respond(
        &self,
        _request: &topaz_kernel::KernelRequest,
        query: &topaz_kernel::HostQuery,
    ) -> topaz_kernel::HostFact {
        match query {
            topaz_kernel::HostQuery::ReadSource { logical_path, .. }
                if logical_path == "root/main.tpz" =>
            {
                topaz_kernel::HostFact::Source(topaz_kernel::SourceFact::Present(
                    self.source.to_string(),
                ))
            }
            topaz_kernel::HostQuery::ReadSource { logical_path, .. }
                if logical_path == "root/lib/sub.tpz" =>
            {
                topaz_kernel::HostFact::Source(topaz_kernel::SourceFact::Present(
                    "export const value = 42\n".to_string(),
                ))
            }
            topaz_kernel::HostQuery::ListDirectory { logical_path, .. }
                if logical_path == "root" =>
            {
                topaz_kernel::HostFact::Directory(topaz_kernel::DirectoryFact::Present(vec![
                    topaz_kernel::DirectoryEntry {
                        name: "lib".to_string(),
                        kind: topaz_kernel::DirectoryEntryKind::Directory,
                    },
                    topaz_kernel::DirectoryEntry {
                        name: "main.tpz".to_string(),
                        kind: topaz_kernel::DirectoryEntryKind::File,
                    },
                ]))
            }
            topaz_kernel::HostQuery::ListDirectory { logical_path, .. }
                if logical_path == "root/lib" =>
            {
                topaz_kernel::HostFact::Directory(topaz_kernel::DirectoryFact::Present(vec![
                    topaz_kernel::DirectoryEntry {
                        name: "sub.tpz".to_string(),
                        kind: topaz_kernel::DirectoryEntryKind::File,
                    },
                ]))
            }
            topaz_kernel::HostQuery::PhysicalContainment { logical_path, .. }
                if matches!(logical_path.as_str(), "root/main.tpz" | "root/lib/sub.tpz") =>
            {
                topaz_kernel::HostFact::Containment(topaz_kernel::ContainmentFact::Inside {
                    alias_class: format!("{}:{logical_path}", self.alias_class),
                })
            }
            _ => panic!("dotted import trivia requested a wrong host fact: {query:?}"),
        }
    }
}

struct CommentBracedImportPrologueResolverHost {
    source: &'static str,
    alias_class: &'static str,
}

impl topaz_kernel::HostFactSource for CommentBracedImportPrologueResolverHost {
    fn respond(
        &self,
        _request: &topaz_kernel::KernelRequest,
        query: &topaz_kernel::HostQuery,
    ) -> topaz_kernel::HostFact {
        match query {
            topaz_kernel::HostQuery::ReadSource { logical_path, .. } => {
                topaz_kernel::HostFact::Source(match logical_path.as_str() {
                    "root/main.tpz" => topaz_kernel::SourceFact::Present(self.source.to_string()),
                    "root/lib.tpz" | "root/other.tpz" => {
                        topaz_kernel::SourceFact::Present("export const value = 42\n".to_string())
                    }
                    _ => topaz_kernel::SourceFact::Missing,
                })
            }
            topaz_kernel::HostQuery::ListDirectory { logical_path, .. } => {
                topaz_kernel::HostFact::Directory(if logical_path == "root" {
                    topaz_kernel::DirectoryFact::Present(vec![
                        topaz_kernel::DirectoryEntry {
                            name: "lib.tpz".to_string(),
                            kind: topaz_kernel::DirectoryEntryKind::File,
                        },
                        topaz_kernel::DirectoryEntry {
                            name: "main.tpz".to_string(),
                            kind: topaz_kernel::DirectoryEntryKind::File,
                        },
                        topaz_kernel::DirectoryEntry {
                            name: "other.tpz".to_string(),
                            kind: topaz_kernel::DirectoryEntryKind::File,
                        },
                    ])
                } else {
                    topaz_kernel::DirectoryFact::Missing
                })
            }
            topaz_kernel::HostQuery::PhysicalContainment { logical_path, .. } => {
                assert!(
                    matches!(
                        logical_path.as_str(),
                        "root/main.tpz" | "root/lib.tpz" | "root/other.tpz"
                    ),
                    "comment-braced import prologue requested an unrelated path: {logical_path}",
                );
                topaz_kernel::HostFact::Containment(topaz_kernel::ContainmentFact::Inside {
                    alias_class: format!("{}:{logical_path}", self.alias_class),
                })
            }
        }
    }
}

struct OutsideImportResolverHost;

impl topaz_kernel::HostFactSource for OutsideImportResolverHost {
    fn respond(
        &self,
        _request: &topaz_kernel::KernelRequest,
        query: &topaz_kernel::HostQuery,
    ) -> topaz_kernel::HostFact {
        match query {
            topaz_kernel::HostQuery::ReadSource { logical_path, .. }
                if logical_path == "root/main.tpz" =>
            {
                topaz_kernel::HostFact::Source(topaz_kernel::SourceFact::Present(
                    RESOLVER_ENTRY_SOURCE.to_string(),
                ))
            }
            topaz_kernel::HostQuery::PhysicalContainment { logical_path, .. }
                if logical_path == "root/main.tpz" =>
            {
                topaz_kernel::HostFact::Containment(topaz_kernel::ContainmentFact::Inside {
                    alias_class: "outside-import:entry".to_string(),
                })
            }
            topaz_kernel::HostQuery::PhysicalContainment { logical_path, .. }
                if logical_path == "root/lib.tpz" =>
            {
                topaz_kernel::HostFact::Containment(topaz_kernel::ContainmentFact::Outside)
            }
            _ => panic!("outside import requested a source or directory fact: {query:?}"),
        }
    }
}

struct MissingFirstSegmentResolverHost;

impl topaz_kernel::HostFactSource for MissingFirstSegmentResolverHost {
    fn respond(
        &self,
        _request: &topaz_kernel::KernelRequest,
        query: &topaz_kernel::HostQuery,
    ) -> topaz_kernel::HostFact {
        match query {
            topaz_kernel::HostQuery::ReadSource { logical_path, .. }
                if logical_path == "root/main.tpz" =>
            {
                topaz_kernel::HostFact::Source(topaz_kernel::SourceFact::Present(
                    NESTED_MISSING_ENTRY_SOURCE.to_string(),
                ))
            }
            topaz_kernel::HostQuery::PhysicalContainment { logical_path, .. }
                if matches!(
                    logical_path.as_str(),
                    "root/main.tpz" | "root/missing/lib.tpz"
                ) =>
            {
                topaz_kernel::HostFact::Containment(topaz_kernel::ContainmentFact::Inside {
                    alias_class: format!("missing-first:{logical_path}"),
                })
            }
            topaz_kernel::HostQuery::ListDirectory { logical_path, .. }
                if logical_path == "root" =>
            {
                topaz_kernel::HostFact::Directory(topaz_kernel::DirectoryFact::Present(vec![
                    topaz_kernel::DirectoryEntry {
                        name: "main.tpz".to_string(),
                        kind: topaz_kernel::DirectoryEntryKind::File,
                    },
                ]))
            }
            _ => panic!("missing first segment requested a deeper path fact: {query:?}"),
        }
    }
}

struct UndeclaredExternResolverHost;

impl topaz_kernel::HostFactSource for UndeclaredExternResolverHost {
    fn respond(
        &self,
        _request: &topaz_kernel::KernelRequest,
        query: &topaz_kernel::HostQuery,
    ) -> topaz_kernel::HostFact {
        match query {
            topaz_kernel::HostQuery::ReadSource { logical_path, .. } => {
                topaz_kernel::HostFact::Source(if logical_path == "root/main.tpz" {
                    topaz_kernel::SourceFact::Present(UNDECLARED_EXTERN_ENTRY_SOURCE.to_string())
                } else {
                    topaz_kernel::SourceFact::Missing
                })
            }
            topaz_kernel::HostQuery::ListDirectory { logical_path, .. } => {
                topaz_kernel::HostFact::Directory(if logical_path == "root" {
                    topaz_kernel::DirectoryFact::Present(vec![topaz_kernel::DirectoryEntry {
                        name: "main.tpz".to_string(),
                        kind: topaz_kernel::DirectoryEntryKind::File,
                    }])
                } else {
                    topaz_kernel::DirectoryFact::Missing
                })
            }
            topaz_kernel::HostQuery::PhysicalContainment { logical_path, .. } => {
                topaz_kernel::HostFact::Containment(topaz_kernel::ContainmentFact::Inside {
                    alias_class: format!("undeclared-extern:{logical_path}"),
                })
            }
        }
    }
}

struct InvalidExternReplayResolverHost;

impl topaz_kernel::HostFactSource for InvalidExternReplayResolverHost {
    fn respond(
        &self,
        _request: &topaz_kernel::KernelRequest,
        query: &topaz_kernel::HostQuery,
    ) -> topaz_kernel::HostFact {
        match query {
            topaz_kernel::HostQuery::ReadSource { logical_path, .. } => {
                topaz_kernel::HostFact::Source(match logical_path.as_str() {
                    "main.tpz" => {
                        topaz_kernel::SourceFact::Present(EXTERN_REPLAY_ENTRY_SOURCE.to_string())
                    }
                    "host/math.tpz" => {
                        topaz_kernel::SourceFact::Present(EXTERN_REPLAY_MODULE_SOURCE.to_string())
                    }
                    _ => topaz_kernel::SourceFact::Missing,
                })
            }
            topaz_kernel::HostQuery::ListDirectory { logical_path, .. } => {
                let entries = match logical_path.as_str() {
                    "" => vec![
                        topaz_kernel::DirectoryEntry {
                            name: "host".to_string(),
                            kind: topaz_kernel::DirectoryEntryKind::Directory,
                        },
                        topaz_kernel::DirectoryEntry {
                            name: "main.tpz".to_string(),
                            kind: topaz_kernel::DirectoryEntryKind::File,
                        },
                    ],
                    "host" => vec![topaz_kernel::DirectoryEntry {
                        name: "math.tpz".to_string(),
                        kind: topaz_kernel::DirectoryEntryKind::File,
                    }],
                    _ => {
                        return topaz_kernel::HostFact::Directory(
                            topaz_kernel::DirectoryFact::Missing,
                        );
                    }
                };
                topaz_kernel::HostFact::Directory(topaz_kernel::DirectoryFact::Present(entries))
            }
            topaz_kernel::HostQuery::PhysicalContainment { logical_path, .. } => {
                topaz_kernel::HostFact::Containment(topaz_kernel::ContainmentFact::Inside {
                    alias_class: format!("invalid-extern-replay:{logical_path}"),
                })
            }
        }
    }
}

struct ReservedTopazResolverHost;

impl topaz_kernel::HostFactSource for ReservedTopazResolverHost {
    fn respond(
        &self,
        _request: &topaz_kernel::KernelRequest,
        query: &topaz_kernel::HostQuery,
    ) -> topaz_kernel::HostFact {
        match query {
            topaz_kernel::HostQuery::ReadSource { logical_path, .. }
                if logical_path == "root/main.tpz" =>
            {
                topaz_kernel::HostFact::Source(topaz_kernel::SourceFact::Present(
                    RESERVED_TOPAZ_ENTRY_SOURCE.to_string(),
                ))
            }
            topaz_kernel::HostQuery::PhysicalContainment { logical_path, .. }
                if matches!(logical_path.as_str(), "root" | "root/main.tpz") =>
            {
                topaz_kernel::HostFact::Containment(topaz_kernel::ContainmentFact::Inside {
                    alias_class: format!("reserved-topaz:{logical_path}"),
                })
            }
            _ => panic!("reserved topaz import requested an unused host fact: {query:?}"),
        }
    }
}

struct OverlappingCycleResolverHost;

impl topaz_kernel::HostFactSource for OverlappingCycleResolverHost {
    fn respond(
        &self,
        _request: &topaz_kernel::KernelRequest,
        query: &topaz_kernel::HostQuery,
    ) -> topaz_kernel::HostFact {
        match query {
            topaz_kernel::HostQuery::ReadSource { logical_path, .. } => {
                let source = match logical_path.as_str() {
                    "root/a.tpz" => Some("import b\n"),
                    "root/b.tpz" => Some("import a\nimport c\n"),
                    "root/c.tpz" => Some("import b\n"),
                    _ => None,
                };
                topaz_kernel::HostFact::Source(match source {
                    Some(value) => topaz_kernel::SourceFact::Present(value.to_string()),
                    None => topaz_kernel::SourceFact::Missing,
                })
            }
            topaz_kernel::HostQuery::ListDirectory { logical_path, .. } => {
                topaz_kernel::HostFact::Directory(if logical_path == "root" {
                    topaz_kernel::DirectoryFact::Present(
                        ["a.tpz", "b.tpz", "c.tpz"]
                            .into_iter()
                            .map(|name| topaz_kernel::DirectoryEntry {
                                name: name.to_string(),
                                kind: topaz_kernel::DirectoryEntryKind::File,
                            })
                            .collect(),
                    )
                } else {
                    topaz_kernel::DirectoryFact::Missing
                })
            }
            topaz_kernel::HostQuery::PhysicalContainment { logical_path, .. } => {
                topaz_kernel::HostFact::Containment(topaz_kernel::ContainmentFact::Inside {
                    alias_class: format!("overlapping-cycle:{logical_path}"),
                })
            }
        }
    }
}

struct ResolverSourcesHost {
    a_source: &'static str,
    b_source: Option<&'static str>,
    c_source: Option<&'static str>,
    alias_class: &'static str,
}

impl topaz_kernel::HostFactSource for ResolverSourcesHost {
    fn respond(
        &self,
        _request: &topaz_kernel::KernelRequest,
        query: &topaz_kernel::HostQuery,
    ) -> topaz_kernel::HostFact {
        match query {
            topaz_kernel::HostQuery::ReadSource { logical_path, .. } => {
                let source = match logical_path.as_str() {
                    "root/a.tpz" | "root/a.tpz.tpz" => Some(self.a_source),
                    "root/b.tpz" => self.b_source,
                    "root/c.tpz" => self.c_source,
                    _ => None,
                };
                topaz_kernel::HostFact::Source(match source {
                    Some(value) => topaz_kernel::SourceFact::Present(value.to_string()),
                    None => topaz_kernel::SourceFact::Missing,
                })
            }
            topaz_kernel::HostQuery::ListDirectory { logical_path, .. } => {
                topaz_kernel::HostFact::Directory(if logical_path == "root" {
                    let mut entries = vec![topaz_kernel::DirectoryEntry {
                        name: "a.tpz".to_string(),
                        kind: topaz_kernel::DirectoryEntryKind::File,
                    }];
                    if self.b_source.is_some() {
                        entries.push(topaz_kernel::DirectoryEntry {
                            name: "b.tpz".to_string(),
                            kind: topaz_kernel::DirectoryEntryKind::File,
                        });
                    }
                    if self.c_source.is_some() {
                        entries.push(topaz_kernel::DirectoryEntry {
                            name: "c.tpz".to_string(),
                            kind: topaz_kernel::DirectoryEntryKind::File,
                        });
                    }
                    topaz_kernel::DirectoryFact::Present(entries)
                } else {
                    topaz_kernel::DirectoryFact::Missing
                })
            }
            topaz_kernel::HostQuery::PhysicalContainment { logical_path, .. } => {
                topaz_kernel::HostFact::Containment(topaz_kernel::ContainmentFact::Inside {
                    alias_class: format!("{}:{logical_path}", self.alias_class),
                })
            }
        }
    }
}

struct PhysicalAliasResolverHost;

impl topaz_kernel::HostFactSource for PhysicalAliasResolverHost {
    fn respond(
        &self,
        _request: &topaz_kernel::KernelRequest,
        query: &topaz_kernel::HostQuery,
    ) -> topaz_kernel::HostFact {
        match query {
            topaz_kernel::HostQuery::ReadSource { logical_path, .. } => {
                topaz_kernel::HostFact::Source(match logical_path.as_str() {
                    "root/a.tpz" => topaz_kernel::SourceFact::Present(
                        "import b\nimport c\nlet value = 1\n".to_string(),
                    ),
                    "root/b.tpz" => {
                        topaz_kernel::SourceFact::Present("export const value = 1\n".to_string())
                    }
                    "root/c.tpz" => topaz_kernel::SourceFact::Present(
                        "import orphan\nexport const value = 1\n".to_string(),
                    ),
                    _ => topaz_kernel::SourceFact::Missing,
                })
            }
            topaz_kernel::HostQuery::ListDirectory { logical_path, .. } => {
                topaz_kernel::HostFact::Directory(if logical_path == "root" {
                    topaz_kernel::DirectoryFact::Present(
                        ["a.tpz", "b.tpz", "c.tpz"]
                            .into_iter()
                            .map(|name| topaz_kernel::DirectoryEntry {
                                name: name.to_string(),
                                kind: topaz_kernel::DirectoryEntryKind::File,
                            })
                            .collect(),
                    )
                } else {
                    topaz_kernel::DirectoryFact::Missing
                })
            }
            topaz_kernel::HostQuery::PhysicalContainment { logical_path, .. } => {
                assert_ne!(
                    logical_path, "root/orphan.tpz",
                    "physical-alias-rejected module requested a descendant host fact",
                );
                let alias_class = match logical_path.as_str() {
                    "root/b.tpz" | "root/c.tpz" => "physical-alias:shared".to_string(),
                    _ => format!("physical-alias:{logical_path}"),
                };
                topaz_kernel::HostFact::Containment(topaz_kernel::ContainmentFact::Inside {
                    alias_class,
                })
            }
        }
    }
}

struct UnreadableDirectoryResolverHost;

impl topaz_kernel::HostFactSource for UnreadableDirectoryResolverHost {
    fn respond(
        &self,
        _request: &topaz_kernel::KernelRequest,
        query: &topaz_kernel::HostQuery,
    ) -> topaz_kernel::HostFact {
        match query {
            topaz_kernel::HostQuery::ReadSource { logical_path, .. } => {
                topaz_kernel::HostFact::Source(match logical_path.as_str() {
                    "root/main.tpz" => topaz_kernel::SourceFact::Present(
                        "import lib\nlet answer = 42\n".to_string(),
                    ),
                    "root/lib.tpz" => {
                        topaz_kernel::SourceFact::Present("export const value = 42\n".to_string())
                    }
                    _ => topaz_kernel::SourceFact::Missing,
                })
            }
            topaz_kernel::HostQuery::ListDirectory { logical_path, .. } => {
                topaz_kernel::HostFact::Directory(if logical_path == "root" {
                    topaz_kernel::DirectoryFact::Unreadable {
                        reason_code: "permission-denied".to_string(),
                    }
                } else {
                    topaz_kernel::DirectoryFact::Missing
                })
            }
            topaz_kernel::HostQuery::PhysicalContainment { logical_path, .. } => {
                topaz_kernel::HostFact::Containment(topaz_kernel::ContainmentFact::Inside {
                    alias_class: format!("unreadable-directory:{logical_path}"),
                })
            }
        }
    }
}

#[derive(Clone, Copy)]
enum UnavailableSourceKind {
    Unreadable,
    InvalidUtf8,
}

struct UnavailableSourceResolverHost {
    entry: bool,
    kind: UnavailableSourceKind,
}

impl UnavailableSourceResolverHost {
    fn unavailable_source(&self) -> topaz_kernel::SourceFact {
        match self.kind {
            UnavailableSourceKind::Unreadable => topaz_kernel::SourceFact::Unreadable {
                reason_code: "permission-denied".to_string(),
            },
            UnavailableSourceKind::InvalidUtf8 => topaz_kernel::SourceFact::InvalidUtf8,
        }
    }
}

impl topaz_kernel::HostFactSource for UnavailableSourceResolverHost {
    fn respond(
        &self,
        _request: &topaz_kernel::KernelRequest,
        query: &topaz_kernel::HostQuery,
    ) -> topaz_kernel::HostFact {
        match query {
            topaz_kernel::HostQuery::ReadSource { logical_path, .. } => {
                let source = match logical_path.as_str() {
                    "root/main.tpz" if self.entry => self.unavailable_source(),
                    "root/main.tpz" => topaz_kernel::SourceFact::Present(
                        "import lib\nlet answer = 42\n".to_string(),
                    ),
                    "root/lib.tpz" if !self.entry => self.unavailable_source(),
                    "root/lib.tpz" => {
                        topaz_kernel::SourceFact::Present("export const value = 42\n".to_string())
                    }
                    _ => topaz_kernel::SourceFact::Missing,
                };
                topaz_kernel::HostFact::Source(source)
            }
            topaz_kernel::HostQuery::ListDirectory { logical_path, .. } => {
                topaz_kernel::HostFact::Directory(if logical_path == "root" {
                    topaz_kernel::DirectoryFact::Present(vec![
                        topaz_kernel::DirectoryEntry {
                            name: "lib.tpz".to_string(),
                            kind: topaz_kernel::DirectoryEntryKind::File,
                        },
                        topaz_kernel::DirectoryEntry {
                            name: "main.tpz".to_string(),
                            kind: topaz_kernel::DirectoryEntryKind::File,
                        },
                    ])
                } else {
                    topaz_kernel::DirectoryFact::Missing
                })
            }
            topaz_kernel::HostQuery::PhysicalContainment { logical_path, .. } => {
                topaz_kernel::HostFact::Containment(topaz_kernel::ContainmentFact::Inside {
                    alias_class: format!("unavailable-source:{logical_path}"),
                })
            }
        }
    }
}

fn stage0_resolved_unit(
    host: &dyn topaz_kernel::HostFactSource,
    request: topaz_kernel::KernelRequest,
) -> Box<topaz_kernel::KernelUnit> {
    match topaz_kernel::drive_checked(host, request).outcome {
        topaz_kernel::KernelOutcome::Completed(unit)
        | topaz_kernel::KernelOutcome::Rejected(unit) => unit,
        topaz_kernel::KernelOutcome::NeedHostFacts(queries) => {
            panic!("Stage 0 left resolver fixture facts pending: {queries:?}")
        }
        topaz_kernel::KernelOutcome::Declined { reason } => {
            panic!("Stage 0 declined resolver fixture: {reason}")
        }
        topaz_kernel::KernelOutcome::ResourceLimit(limit) => {
            panic!("Stage 0 exhausted resolver fixture resource: {limit:?}")
        }
        topaz_kernel::KernelOutcome::CompilerFault { message } => {
            panic!("Stage 0 resolver fixture compiler fault: {message}")
        }
    }
}

struct UnicodeResolverFixtureHost;

impl topaz_kernel::HostFactSource for UnicodeResolverFixtureHost {
    fn respond(
        &self,
        _request: &topaz_kernel::KernelRequest,
        query: &topaz_kernel::HostQuery,
    ) -> topaz_kernel::HostFact {
        match query {
                topaz_kernel::HostQuery::ReadSource { logical_path, .. } => {
                    topaz_kernel::HostFact::Source(match logical_path.as_str() {
                        "root/main.tpz" => topaz_kernel::SourceFact::Present(
                            "import 한글.모듈 { 값 }\nimport модуль { значение }\nimport 🚀 { 발사 }\nlet 합계 = 값 + значение + 발사\n"
                                .to_string(),
                        ),
                        "root/한글/모듈.tpz" => topaz_kernel::SourceFact::Present(
                            "export const 값 = 20\n".to_string(),
                        ),
                        "root/модуль.tpz" => topaz_kernel::SourceFact::Present(
                            "export const значение = 21\n".to_string(),
                        ),
                        "root/🚀.tpz" => topaz_kernel::SourceFact::Present(
                            "export const 발사 = 1\n".to_string(),
                        ),
                        _ => topaz_kernel::SourceFact::Missing,
                    })
                }
                topaz_kernel::HostQuery::ListDirectory { logical_path, .. } => {
                    let entries = match logical_path.as_str() {
                        "root" => vec![
                            topaz_kernel::DirectoryEntry {
                                name: "main.tpz".to_string(),
                                kind: topaz_kernel::DirectoryEntryKind::File,
                            },
                            topaz_kernel::DirectoryEntry {
                                name: "модуль.tpz".to_string(),
                                kind: topaz_kernel::DirectoryEntryKind::File,
                            },
                            topaz_kernel::DirectoryEntry {
                                name: "한글".to_string(),
                                kind: topaz_kernel::DirectoryEntryKind::Directory,
                            },
                            topaz_kernel::DirectoryEntry {
                                name: "🚀.tpz".to_string(),
                                kind: topaz_kernel::DirectoryEntryKind::File,
                            },
                        ],
                        "root/한글" => vec![topaz_kernel::DirectoryEntry {
                            name: "모듈.tpz".to_string(),
                            kind: topaz_kernel::DirectoryEntryKind::File,
                        }],
                        _ => {
                            return topaz_kernel::HostFact::Directory(
                                topaz_kernel::DirectoryFact::Missing,
                            );
                        }
                    };
                    topaz_kernel::HostFact::Directory(topaz_kernel::DirectoryFact::Present(entries))
                }
                topaz_kernel::HostQuery::PhysicalContainment { logical_path, .. } => {
                    topaz_kernel::HostFact::Containment(topaz_kernel::ContainmentFact::Inside {
                        alias_class: format!("unicode:{logical_path}"),
                    })
                }
            }
    }
}

#[derive(Clone, Copy)]
enum UnicodeCollisionKind {
    CaseFold,
    Canonical,
}

struct UnicodeCollisionResolverHost(UnicodeCollisionKind);

impl topaz_kernel::HostFactSource for UnicodeCollisionResolverHost {
    fn respond(
        &self,
        _request: &topaz_kernel::KernelRequest,
        query: &topaz_kernel::HostQuery,
    ) -> topaz_kernel::HostFact {
        let (entry_source, exact_path, first_name, second_name) = match self.0 {
            UnicodeCollisionKind::CaseFold => (
                "import straße { value }\nlet answer = value\n",
                "root/straße.tpz",
                "strasse.tpz",
                "straße.tpz",
            ),
            UnicodeCollisionKind::Canonical => (
                "import café { value }\nlet answer = value\n",
                "root/café.tpz",
                "cafe\u{301}.tpz",
                "café.tpz",
            ),
        };
        match query {
            topaz_kernel::HostQuery::ReadSource { logical_path, .. } => {
                topaz_kernel::HostFact::Source(if logical_path == "root/main.tpz" {
                    topaz_kernel::SourceFact::Present(entry_source.to_string())
                } else if logical_path == exact_path {
                    topaz_kernel::SourceFact::Present("export const value = 42\n".to_string())
                } else {
                    topaz_kernel::SourceFact::Missing
                })
            }
            topaz_kernel::HostQuery::ListDirectory { logical_path, .. } => {
                topaz_kernel::HostFact::Directory(if logical_path == "root" {
                    topaz_kernel::DirectoryFact::Present(vec![
                        topaz_kernel::DirectoryEntry {
                            name: "main.tpz".to_string(),
                            kind: topaz_kernel::DirectoryEntryKind::File,
                        },
                        topaz_kernel::DirectoryEntry {
                            name: first_name.to_string(),
                            kind: topaz_kernel::DirectoryEntryKind::File,
                        },
                        topaz_kernel::DirectoryEntry {
                            name: second_name.to_string(),
                            kind: topaz_kernel::DirectoryEntryKind::File,
                        },
                    ])
                } else {
                    topaz_kernel::DirectoryFact::Missing
                })
            }
            topaz_kernel::HostQuery::PhysicalContainment { logical_path, .. } => {
                topaz_kernel::HostFact::Containment(topaz_kernel::ContainmentFact::Inside {
                    alias_class: format!("unicode-collision:{logical_path}"),
                })
            }
        }
    }
}

struct BootstrapFixtureHost {
    root: std::path::PathBuf,
}

impl BootstrapFixtureHost {
    fn new() -> Self {
        Self {
            root: std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../corpus/v5_10/bootstrap-workload"),
        }
    }
}

impl topaz_kernel::HostFactSource for BootstrapFixtureHost {
    fn respond(
        &self,
        _request: &topaz_kernel::KernelRequest,
        query: &topaz_kernel::HostQuery,
    ) -> topaz_kernel::HostFact {
        let logical_path = query.logical_path();
        let path = self.root.join(logical_path);
        match query {
            topaz_kernel::HostQuery::ReadSource { .. } => {
                topaz_kernel::HostFact::Source(match std::fs::read_to_string(path) {
                    Ok(source) => topaz_kernel::SourceFact::Present(source),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        topaz_kernel::SourceFact::Missing
                    }
                    Err(error) => topaz_kernel::SourceFact::Unreadable {
                        reason_code: error.kind().to_string(),
                    },
                })
            }
            topaz_kernel::HostQuery::ListDirectory { .. } => {
                let entries = match std::fs::read_dir(path) {
                    Ok(entries) => entries,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        return topaz_kernel::HostFact::Directory(
                            topaz_kernel::DirectoryFact::Missing,
                        );
                    }
                    Err(error) => {
                        return topaz_kernel::HostFact::Directory(
                            topaz_kernel::DirectoryFact::Unreadable {
                                reason_code: error.kind().to_string(),
                            },
                        );
                    }
                };
                let mut values = entries
                    .filter_map(Result::ok)
                    .map(|entry| topaz_kernel::DirectoryEntry {
                        name: entry.file_name().to_string_lossy().to_string(),
                        kind: if entry.path().is_dir() {
                            topaz_kernel::DirectoryEntryKind::Directory
                        } else {
                            topaz_kernel::DirectoryEntryKind::File
                        },
                    })
                    .collect::<Vec<_>>();
                values.sort_by(|left, right| left.name.cmp(&right.name));
                topaz_kernel::HostFact::Directory(topaz_kernel::DirectoryFact::Present(values))
            }
            topaz_kernel::HostQuery::PhysicalContainment { .. } => {
                topaz_kernel::HostFact::Containment(topaz_kernel::ContainmentFact::Inside {
                    alias_class: format!("bootstrap:{logical_path}"),
                })
            }
        }
    }
}

struct TypeMismatchFixtureHost;

impl topaz_kernel::HostFactSource for TypeMismatchFixtureHost {
    fn respond(
        &self,
        _request: &topaz_kernel::KernelRequest,
        query: &topaz_kernel::HostQuery,
    ) -> topaz_kernel::HostFact {
        match query {
            topaz_kernel::HostQuery::ReadSource { logical_path, .. } => {
                topaz_kernel::HostFact::Source(if logical_path == "main.tpz" {
                    topaz_kernel::SourceFact::Present("let answer: int = \"no\"\n".to_string())
                } else {
                    topaz_kernel::SourceFact::Missing
                })
            }
            topaz_kernel::HostQuery::ListDirectory { logical_path, .. } => {
                topaz_kernel::HostFact::Directory(if logical_path.is_empty() {
                    topaz_kernel::DirectoryFact::Present(vec![topaz_kernel::DirectoryEntry {
                        name: "main.tpz".to_string(),
                        kind: topaz_kernel::DirectoryEntryKind::File,
                    }])
                } else {
                    topaz_kernel::DirectoryFact::Missing
                })
            }
            topaz_kernel::HostQuery::PhysicalContainment { logical_path, .. } => {
                topaz_kernel::HostFact::Containment(topaz_kernel::ContainmentFact::Inside {
                    alias_class: format!("mismatch:{logical_path}"),
                })
            }
        }
    }
}

struct SourceFixtureHost<'a>(&'a str);

impl topaz_kernel::HostFactSource for SourceFixtureHost<'_> {
    fn respond(
        &self,
        _request: &topaz_kernel::KernelRequest,
        query: &topaz_kernel::HostQuery,
    ) -> topaz_kernel::HostFact {
        match query {
            topaz_kernel::HostQuery::ReadSource { logical_path, .. } => {
                topaz_kernel::HostFact::Source(if logical_path == "main.tpz" {
                    topaz_kernel::SourceFact::Present(self.0.to_string())
                } else {
                    topaz_kernel::SourceFact::Missing
                })
            }
            topaz_kernel::HostQuery::ListDirectory { logical_path, .. } => {
                topaz_kernel::HostFact::Directory(if logical_path.is_empty() {
                    topaz_kernel::DirectoryFact::Present(vec![topaz_kernel::DirectoryEntry {
                        name: "main.tpz".to_string(),
                        kind: topaz_kernel::DirectoryEntryKind::File,
                    }])
                } else {
                    topaz_kernel::DirectoryFact::Missing
                })
            }
            topaz_kernel::HostQuery::PhysicalContainment { logical_path, .. } => {
                topaz_kernel::HostFact::Containment(topaz_kernel::ContainmentFact::Inside {
                    alias_class: format!("source-fixture:{logical_path}"),
                })
            }
        }
    }
}

struct ReceiverMethodImportFixtureHost;

impl topaz_kernel::HostFactSource for ReceiverMethodImportFixtureHost {
    fn respond(
        &self,
        _request: &topaz_kernel::KernelRequest,
        query: &topaz_kernel::HostQuery,
    ) -> topaz_kernel::HostFact {
        match query {
            topaz_kernel::HostQuery::ReadSource { logical_path, .. } => {
                topaz_kernel::HostFact::Source(match logical_path.as_str() {
                    "root/main.tpz" => topaz_kernel::SourceFact::Present(
                        concat!(
                            "import model { Point }\n",
                            "let moved: Point = Point { value: 40 }.shifted(2)\n",
                            "let piped: Point = 3 |> moved.shifted()\n",
                        )
                        .to_string(),
                    ),
                    "root/model.tpz" => topaz_kernel::SourceFact::Present(
                        concat!(
                            "export record Point { value: int }\n",
                            "impl Point {\n",
                            "  export function shifted(self, delta: int) -> Point {\n",
                            "    Point { value: self.value + delta }\n",
                            "  }\n",
                            "  function hidden(self) -> int { self.value }\n",
                            "}\n",
                        )
                        .to_string(),
                    ),
                    _ => topaz_kernel::SourceFact::Missing,
                })
            }
            topaz_kernel::HostQuery::ListDirectory { logical_path, .. } => {
                topaz_kernel::HostFact::Directory(if logical_path == "root" {
                    topaz_kernel::DirectoryFact::Present(
                        ["main.tpz", "model.tpz"]
                            .into_iter()
                            .map(|name| topaz_kernel::DirectoryEntry {
                                name: name.to_string(),
                                kind: topaz_kernel::DirectoryEntryKind::File,
                            })
                            .collect(),
                    )
                } else {
                    topaz_kernel::DirectoryFact::Missing
                })
            }
            topaz_kernel::HostQuery::PhysicalContainment { logical_path, .. } => {
                topaz_kernel::HostFact::Containment(topaz_kernel::ContainmentFact::Inside {
                    alias_class: format!("receiver-method-import:{logical_path}"),
                })
            }
        }
    }
}

struct CaptureImportFixtureHost;

impl topaz_kernel::HostFactSource for CaptureImportFixtureHost {
    fn respond(
        &self,
        _request: &topaz_kernel::KernelRequest,
        query: &topaz_kernel::HostQuery,
    ) -> topaz_kernel::HostFact {
        match query {
                topaz_kernel::HostQuery::ReadSource { logical_path, .. } => {
                    topaz_kernel::HostFact::Source(match logical_path.as_str() {
                        "root/main.tpz" => topaz_kernel::SourceFact::Present(
                            concat!(
                                "import lib { value, apply, Marker }\n",
                                "import space as ns\n",
                                "let alias = apply\n",
                                "let selected = () => value\n",
                                "let namespaced = () => ns.value\n",
                                "let called = () => alias(amount: 2, value: 1)\n",
                                "let typedOnly = () => {\n",
                                "  let item: Marker = Marker { value: 1 }\n",
                                "  item.value\n",
                                "}\n",
                            )
                            .to_string(),
                        ),
                        "root/lib.tpz" => topaz_kernel::SourceFact::Present(
                            concat!(
                                "export let value = 40\n",
                                "export function apply(value: int, amount: int = 1) -> int { value + amount }\n",
                                "export record Marker { value: int }\n",
                            )
                            .to_string(),
                        ),
                        "root/space.tpz" => topaz_kernel::SourceFact::Present(
                            "export let value = 41\n".to_string(),
                        ),
                        _ => topaz_kernel::SourceFact::Missing,
                    })
                }
                topaz_kernel::HostQuery::ListDirectory { logical_path, .. } => {
                    topaz_kernel::HostFact::Directory(if logical_path == "root" {
                        topaz_kernel::DirectoryFact::Present(vec![
                            topaz_kernel::DirectoryEntry {
                                name: "lib.tpz".to_string(),
                                kind: topaz_kernel::DirectoryEntryKind::File,
                            },
                            topaz_kernel::DirectoryEntry {
                                name: "main.tpz".to_string(),
                                kind: topaz_kernel::DirectoryEntryKind::File,
                            },
                            topaz_kernel::DirectoryEntry {
                                name: "space.tpz".to_string(),
                                kind: topaz_kernel::DirectoryEntryKind::File,
                            },
                        ])
                    } else {
                        topaz_kernel::DirectoryFact::Missing
                    })
                }
                topaz_kernel::HostQuery::PhysicalContainment { logical_path, .. } => {
                    topaz_kernel::HostFact::Containment(
                        topaz_kernel::ContainmentFact::Inside {
                            alias_class: format!("capture-fixture:{logical_path}"),
                        },
                    )
                }
            }
    }
}

fn typed_source(source: &str) -> TypedPreviewResult {
    let request = topaz_kernel::KernelRequest::checked(
        "main.tpz",
        Some(""),
        LangVersion::CURRENT,
        topaz_kernel::PackageFacts::standalone(),
    )
    .with_terminal_phase(topaz_kernel::TerminalPhase::Typed);
    preview_typed(&SourceFixtureHost(source), request).expect("typed source preview")
}

fn stage0_typed_captures(
    source: &dyn topaz_kernel::HostFactSource,
    request: topaz_kernel::KernelRequest,
) -> Vec<topaz_hir::TypedCapture> {
    let execution = topaz_kernel::drive_checked(source, request);
    let unit = match execution.outcome {
        topaz_kernel::KernelOutcome::Completed(unit) => unit,
        topaz_kernel::KernelOutcome::Rejected(unit) => {
            panic!(
                "Stage 0 rejected capture fixture: resolved={:?}, checked={:?}",
                unit.resolved.diagnostics,
                unit.checked.as_ref().map(|checked| &checked.diagnostics)
            )
        }
        topaz_kernel::KernelOutcome::NeedHostFacts(queries) => {
            panic!("Stage 0 left capture fixture queries pending: {queries:?}")
        }
        topaz_kernel::KernelOutcome::Declined { reason } => {
            panic!("Stage 0 declined capture fixture: {reason}")
        }
        topaz_kernel::KernelOutcome::ResourceLimit(limit) => {
            panic!("Stage 0 exhausted capture fixture resource: {limit:?}")
        }
        topaz_kernel::KernelOutcome::CompilerFault { message } => {
            panic!("Stage 0 capture fixture compiler fault: {message}")
        }
    };
    unit.checked
        .and_then(|checked| checked.typed_hir)
        .expect("Stage 0 typed capture fixture")
        .captures
}

fn stage0_typed_calls(source: &str) -> Vec<topaz_hir::TypedCall> {
    let request = topaz_kernel::KernelRequest::checked(
        "main.tpz",
        Some(""),
        LangVersion::CURRENT,
        topaz_kernel::PackageFacts::standalone(),
    )
    .with_terminal_phase(topaz_kernel::TerminalPhase::Typed);
    stage0_resolved_unit(&SourceFixtureHost(source), request)
        .checked
        .and_then(|checked| checked.typed_hir)
        .expect("Stage 0 typed call fixture")
        .calls
}

fn canonical_capture_files(
    mut captures: Vec<topaz_hir::TypedCapture>,
) -> Vec<topaz_hir::TypedCapture> {
    for capture in &mut captures {
        capture.closure_span.file = FileId(0);
        capture.reference_span.file = FileId(0);
        capture.declaration_span.file = FileId(0);
    }
    captures
}

fn self_checker_diagnostics(source: &str) -> Vec<(String, String, u32, u32)> {
    let preview = typed_source(source);
    assert!(
        preview.resolved.diagnostics.is_empty(),
        "self front end did not resolve fixture: {:?}",
        preview.resolved.diagnostics,
    );
    preview
        .diagnostics
        .into_iter()
        .map(|diagnostic| {
            (
                diagnostic.code,
                diagnostic.message,
                diagnostic.lo,
                diagnostic.hi,
            )
        })
        .collect()
}

fn stage0_checker_diagnostics(source: &str) -> Vec<(String, String, u32, u32)> {
    let parsed = topaz_parser::parse_with_options(
        FileId(0),
        source,
        topaz_parser::ParseOptions {
            language_version: LangVersion::CURRENT,
        },
    );
    assert!(
        parsed.diagnostics.is_empty(),
        "Stage 0 did not parse fixture: {:?}",
        parsed.diagnostics,
    );
    topaz_check::check_program_with_version(source, &parsed.program, LangVersion::CURRENT)
        .diagnostics
        .into_iter()
        .map(|diagnostic| {
            (
                diagnostic.code.as_str().to_string(),
                diagnostic.message,
                diagnostic.primary.span.lo,
                diagnostic.primary.span.hi,
            )
        })
        .collect()
}

fn stage0_unit_checker_diagnostics(source: &str) -> Vec<(String, String, u32, u32)> {
    let parsed = topaz_parser::parse_with_options(
        FileId(0),
        source,
        topaz_parser::ParseOptions {
            language_version: LangVersion::CURRENT,
        },
    );
    assert!(
        parsed.diagnostics.is_empty(),
        "Stage 0 did not parse fixture: {:?}",
        parsed.diagnostics,
    );
    topaz_check::check_unit_with_version(
        &[topaz_check::UnitModule {
            identity: "main".to_string(),
            is_entry: true,
            is_extern: false,
            is_generated_std: false,
            extern_replay_error: None,
            src: source,
            program: &parsed.program,
        }],
        LangVersion::CURRENT,
    )
    .diagnostics
    .into_iter()
    .map(|diagnostic| {
        (
            diagnostic.code.as_str().to_string(),
            diagnostic.message,
            diagnostic.primary.span.lo,
            diagnostic.primary.span.hi,
        )
    })
    .collect()
}

fn resolver_request() -> topaz_kernel::KernelRequest {
    topaz_kernel::KernelRequest::checked(
        "root/main.tpz",
        Some("root"),
        LangVersion::CURRENT,
        topaz_kernel::PackageFacts::standalone(),
    )
    .with_terminal_phase(topaz_kernel::TerminalPhase::Resolved)
}

fn resolved_preview_bundle(preview: &ResolvedPreviewResult) -> topaz_kernel::ObservationBundle {
    topaz_kernel::build_resolved_preview_observation(preview.observation_input())
        .expect("resolved preview observation")
}

fn preview_stream(
    root: &std::collections::BTreeMap<String, JsonValue>,
    stream: &str,
) -> Vec<(String, u32, u32)> {
    let JsonValue::Array(tokens) = root.get(stream).expect("stream") else {
        panic!("stream must be an array");
    };
    tokens
        .iter()
        .map(|token| {
            let JsonValue::Object(token) = token else {
                panic!("token must be an object");
            };
            let JsonValue::String(kind) = token.get("kind").expect("kind") else {
                panic!("kind must be a string");
            };
            let JsonValue::Number(lo) = token.get("lo").expect("lo") else {
                panic!("lo must be a number");
            };
            let JsonValue::Number(hi) = token.get("hi").expect("hi") else {
                panic!("hi must be a number");
            };
            (
                kind.to_string(),
                u32::try_from(lo.int.expect("integer lo")).expect("u32 lo"),
                u32::try_from(hi.int.expect("integer hi")).expect("u32 hi"),
            )
        })
        .collect()
}

fn preview_response(
    session: &FrontEndSession,
    source: &str,
) -> std::collections::BTreeMap<String, JsonValue> {
    let encoded_source = json_stringify(&Value::str(source), true).expect("encode fixture source");
    let request = format!(
        "{{\"schema\":\"{EXCHANGE_SCHEMA}\",\"terminal\":\"ast\",\"entry\":\"fixture\",\
             \"root\":\"\",\"source\":{encoded_source},\"sourceId\":\"fixture\",\"facts\":[],\
             \"package\":{{\"buildRole\":\"standalone\",\"externModules\":[],\
             \"externReplayModules\":[],\"externReplayErrors\":[],\"generatedStdModules\":[]}},\
             \"maxAstNodes\":{MAX_AST_NODES},\"maxAstDepth\":{MAX_AST_DEPTH}}}"
    );
    let response = session
        .invoke(request.as_bytes())
        .expect("preview exchange");
    let parsed =
        json_parse(std::str::from_utf8(&response).expect("response UTF-8")).expect("response JSON");
    let JsonValue::Object(root) = parsed else {
        panic!("response root must be an object");
    };
    root.iter()
        .map(|(key, value)| (key.to_string(), value.clone()))
        .collect()
}

fn preview_diagnostics(
    root: &std::collections::BTreeMap<String, JsonValue>,
) -> Vec<(String, String, u32, u32)> {
    let JsonValue::Array(diagnostics) = root.get("diagnostics").expect("diagnostics") else {
        panic!("diagnostics must be an array");
    };
    diagnostics
        .iter()
        .map(|diagnostic| {
            let JsonValue::Object(diagnostic) = diagnostic else {
                panic!("diagnostic must be an object");
            };
            let JsonValue::String(code) = diagnostic.get("code").expect("code") else {
                panic!("code must be a string");
            };
            let JsonValue::String(message) = diagnostic.get("message").expect("message") else {
                panic!("message must be a string");
            };
            let JsonValue::Number(lo) = diagnostic.get("lo").expect("lo") else {
                panic!("lo must be a number");
            };
            let JsonValue::Number(hi) = diagnostic.get("hi").expect("hi") else {
                panic!("hi must be a number");
            };
            (
                code.to_string(),
                message.to_string(),
                u32::try_from(lo.int.expect("integer lo")).expect("u32 lo"),
                u32::try_from(hi.int.expect("integer hi")).expect("u32 hi"),
            )
        })
        .collect()
}

fn rust_diagnostics(source: &str) -> Vec<(String, String, u32, u32)> {
    let raw = lex(FileId(0), source);
    let layout = normalize_with_options(
        &raw.tokens,
        source,
        LayoutOptions {
            language_version: LangVersion::CURRENT,
        },
    );
    raw.diagnostics
        .iter()
        .chain(layout.diagnostics.iter())
        .map(|diagnostic| {
            (
                diagnostic.code.as_str().to_string(),
                diagnostic.message.clone(),
                diagnostic.primary.span.lo,
                diagnostic.primary.span.hi,
            )
        })
        .collect()
}

fn rust_frontend_diagnostics(source: &str) -> Vec<(String, String, u32, u32)> {
    topaz_parser::parse_with_options(
        FileId(0),
        source,
        topaz_parser::ParseOptions {
            language_version: LangVersion::CURRENT,
        },
    )
    .diagnostics
    .into_iter()
    .map(|diagnostic| {
        (
            diagnostic.code.as_str().to_string(),
            diagnostic.message,
            diagnostic.primary.span.lo,
            diagnostic.primary.span.hi,
        )
    })
    .collect()
}

fn rust_stream(source: &str, layout: bool) -> Vec<(String, u32, u32)> {
    let raw = lex(FileId(0), source);
    assert!(raw.diagnostics.is_empty(), "{:?}", raw.diagnostics);
    let tokens = if layout {
        normalize_with_options(
            &raw.tokens,
            source,
            LayoutOptions {
                language_version: LangVersion::CURRENT,
            },
        )
        .tokens
    } else {
        raw.tokens
    };
    tokens
        .iter()
        .map(|token| {
            (
                topaz_kernel::canonical_token_kind(token.kind),
                token.span.lo,
                token.span.hi,
            )
        })
        .collect()
}

struct LoweringFixtureHost;

impl topaz_kernel::HostFactSource for LoweringFixtureHost {
    fn respond(
        &self,
        _request: &topaz_kernel::KernelRequest,
        query: &topaz_kernel::HostQuery,
    ) -> topaz_kernel::HostFact {
        match query {
            topaz_kernel::HostQuery::ReadSource { logical_path, .. } => {
                topaz_kernel::HostFact::Source(if logical_path == "root/main.tpz" {
                    topaz_kernel::SourceFact::Present("let answer = 40 + 2\n".to_string())
                } else {
                    topaz_kernel::SourceFact::Missing
                })
            }
            topaz_kernel::HostQuery::ListDirectory { logical_path, .. } => {
                topaz_kernel::HostFact::Directory(if logical_path == "root" {
                    topaz_kernel::DirectoryFact::Present(vec![topaz_kernel::DirectoryEntry {
                        name: "main.tpz".to_string(),
                        kind: topaz_kernel::DirectoryEntryKind::File,
                    }])
                } else {
                    topaz_kernel::DirectoryFact::Missing
                })
            }
            topaz_kernel::HostQuery::PhysicalContainment { logical_path, .. } => {
                topaz_kernel::HostFact::Containment(topaz_kernel::ContainmentFact::Inside {
                    alias_class: logical_path.clone(),
                })
            }
        }
    }
}

struct InlineLoweringFixtureHost<'a>(&'a str);

impl topaz_kernel::HostFactSource for InlineLoweringFixtureHost<'_> {
    fn respond(
        &self,
        _request: &topaz_kernel::KernelRequest,
        query: &topaz_kernel::HostQuery,
    ) -> topaz_kernel::HostFact {
        match query {
            topaz_kernel::HostQuery::ReadSource { logical_path, .. } => {
                topaz_kernel::HostFact::Source(if logical_path == "root/main.tpz" {
                    topaz_kernel::SourceFact::Present(self.0.to_string())
                } else {
                    topaz_kernel::SourceFact::Missing
                })
            }
            topaz_kernel::HostQuery::ListDirectory { logical_path, .. } => {
                topaz_kernel::HostFact::Directory(if logical_path == "root" {
                    topaz_kernel::DirectoryFact::Present(vec![topaz_kernel::DirectoryEntry {
                        name: "main.tpz".to_string(),
                        kind: topaz_kernel::DirectoryEntryKind::File,
                    }])
                } else {
                    topaz_kernel::DirectoryFact::Missing
                })
            }
            topaz_kernel::HostQuery::PhysicalContainment { logical_path, .. } => {
                topaz_kernel::HostFact::Containment(topaz_kernel::ContainmentFact::Inside {
                    alias_class: logical_path.clone(),
                })
            }
        }
    }
}

fn lowering_request() -> topaz_kernel::KernelRequest {
    topaz_kernel::KernelRequest::checked(
        "root/main.tpz",
        Some("root"),
        LangVersion::CURRENT,
        topaz_kernel::PackageFacts::standalone(),
    )
    .with_terminal_phase(topaz_kernel::TerminalPhase::Lowered)
}

fn generated_request() -> topaz_kernel::KernelRequest {
    topaz_kernel::KernelRequest::checked(
        "root/main.tpz",
        Some("root"),
        LangVersion::CURRENT,
        topaz_kernel::PackageFacts::standalone(),
    )
    .with_terminal_phase(topaz_kernel::TerminalPhase::RustSource)
}

mod agreement;
mod manifest;
mod preview;
mod product;
mod regressions;
mod source;
mod stage1;
mod typed;
