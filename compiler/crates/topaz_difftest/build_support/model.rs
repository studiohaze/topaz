use topaz_syntax::LangVersion;

pub(crate) struct BoxedFixtureDef {
    pub(crate) name: &'static str,
    pub(crate) source: &'static str,
    pub(crate) source_path: &'static str,
}

pub(crate) type ModuleFixtureDef = (
    &'static str,
    &'static str,
    &'static [(&'static str, &'static str)],
);

pub(crate) type VersionedModuleFixtureDef = (
    &'static str,
    &'static str,
    LangVersion,
    &'static [(&'static str, &'static str)],
);

pub(crate) type ExternFixtureDef = (
    &'static str,
    &'static str,
    &'static str,
    Option<&'static str>,
);

pub(crate) type ExternModuleFixtureDef = (
    &'static str,
    &'static str,
    &'static [(&'static str, &'static str)],
    &'static [ExternFixtureDef],
    &'static str,
);
