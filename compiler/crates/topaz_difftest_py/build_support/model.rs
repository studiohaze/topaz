#[derive(Debug, Clone, Copy)]
pub(crate) enum WideKind {
    Regular,
    FileConfig,
}

impl WideKind {
    pub(crate) fn generated_name(self) -> &'static str {
        match self {
            Self::Regular => "WideCoreKind::Regular",
            Self::FileConfig => "WideCoreKind::FileConfig",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct WideFixture {
    pub(crate) name: &'static str,
    pub(crate) source: &'static str,
    pub(crate) source_path: &'static str,
    pub(crate) kind: WideKind,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct FixtureFile {
    pub(crate) path: &'static str,
    pub(crate) source: &'static str,
    pub(crate) source_path: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ModuleFixture {
    pub(crate) name: &'static str,
    pub(crate) entry: &'static str,
    pub(crate) files: &'static [FixtureFile],
}
