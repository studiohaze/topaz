// Standalone source mode has no package manifest. Project builds replace this
// file with the explicit grants carried by topaz.toml or a saved project input.
pub fn host() -> Result<topaz_host_native::NativeHost, String> {
    Ok(topaz_host_native::NativeHost::new())
}
