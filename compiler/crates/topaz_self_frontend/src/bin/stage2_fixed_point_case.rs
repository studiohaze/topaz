use std::ffi::OsString;
use std::path::PathBuf;

struct SingleFileHost<'source> {
    source: &'source str,
}

impl topaz_kernel::HostFactSource for SingleFileHost<'_> {
    fn respond(
        &self,
        _request: &topaz_kernel::KernelRequest,
        query: &topaz_kernel::HostQuery,
    ) -> topaz_kernel::HostFact {
        match query {
            topaz_kernel::HostQuery::ReadSource { logical_path, .. } => {
                topaz_kernel::HostFact::Source(if logical_path == "main.tpz" {
                    topaz_kernel::SourceFact::Present(self.source.to_owned())
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
                    alias_class: format!("stage2-verification:{logical_path}"),
                })
            }
        }
    }
}

fn sha256(bytes: &[u8]) -> String {
    let digest = topaz_value::value::sha256(bytes);
    let mut hex = String::new();
    topaz_value::bytes_to_hex_into(&mut hex, &digest);
    format!("sha256:{hex}")
}

fn parse_arguments(arguments: impl IntoIterator<Item = OsString>) -> Result<PathBuf, String> {
    let mut arguments = arguments.into_iter();
    let input = arguments
        .next()
        .ok_or_else(|| "stage2_fixed_point_case requires one input path".to_string())?;
    if let Some(argument) = arguments.next() {
        return Err(format!(
            "stage2_fixed_point_case accepts one input path, got extra `{}`",
            argument.to_string_lossy()
        ));
    }
    Ok(PathBuf::from(input))
}

fn main() -> Result<(), String> {
    let input = parse_arguments(std::env::args_os().skip(1))?;
    let source = std::fs::read_to_string(&input)
        .map_err(|error| format!("cannot read {}: {error}", input.display()))?;
    let host = SingleFileHost { source: &source };
    let request = || {
        topaz_kernel::KernelRequest::checked(
            "main.tpz",
            Some(""),
            topaz_syntax::LangVersion::CURRENT,
            topaz_kernel::PackageFacts::standalone(),
        )
        .with_terminal_phase(topaz_kernel::TerminalPhase::Lowered)
    };
    let c1 = topaz_self_frontend::preview_linked_stage1_lowered(&host, request())?;
    let c2 = topaz_self_frontend::preview_linked_stage2_lowered(&host, request())?;
    if c1.front_end != c2.front_end || c1.status != c2.status {
        return Err("C1/C2 canonical front-end observation differs".to_string());
    }
    println!(
        concat!(
            "{{",
            "\"schema\":\"topaz.compiler.stage2-fixed-point-case/v1\",",
            "\"inputSha256\":\"{}\",",
            "\"status\":\"{}\",",
            "\"frontEndSha256\":\"{}\",",
            "\"semanticEqual\":true,",
            "\"c1Rounds\":{},",
            "\"c2Rounds\":{}",
            "}}"
        ),
        sha256(source.as_bytes()),
        c1.status,
        sha256(c1.front_end.as_bytes()),
        c1.rounds,
        c2.rounds,
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_exactly_one_input_path() {
        assert_eq!(
            parse_arguments([OsString::from("fixture.tpz")]),
            Ok(PathBuf::from("fixture.tpz"))
        );
    }

    #[test]
    fn rejects_missing_and_extra_input_paths() {
        assert_eq!(
            parse_arguments([]),
            Err("stage2_fixed_point_case requires one input path".to_string())
        );
        assert_eq!(
            parse_arguments([OsString::from("first.tpz"), OsString::from("second.tpz")]),
            Err(
                "stage2_fixed_point_case accepts one input path, got extra `second.tpz`"
                    .to_string()
            )
        );
    }
}
