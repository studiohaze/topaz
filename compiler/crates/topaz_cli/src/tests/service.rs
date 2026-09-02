use super::*;
use std::time::{SystemTime, UNIX_EPOCH};

struct TempService(PathBuf);

impl Drop for TempService {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn service_package(source: &str) -> (TempService, PackageTarget) {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "topaz-http-service-contract-{}-{nanos}",
        std::process::id()
    ));
    fs::create_dir_all(root.join("src")).expect("service src");
    fs::write(
            root.join("topaz.toml"),
            "[package]\nname = \"service_contract\"\nversion = \"0.1.0\"\nlanguage = \"5.9\"\nentry = \"src/main.tpz\"\n\n[build]\ntarget = \"http-service\"\n",
        )
        .expect("manifest");
    fs::write(root.join("src/main.tpz"), source).expect("entry");
    let root_arg = root.to_string_lossy().into_owned();
    let target = package_target(Some(&root_arg), None, false).expect("service target");
    (TempService(root), target)
}

#[test]
fn service_lowering_keeps_loop_checkpoints_and_deadline_call_seam() {
    let (_package, target) = service_package(
        "import std.http { HttpRequest, HttpResponse, text }\n\nexport function handle(req: HttpRequest) -> HttpResponse {\n  while true { }\n  text(200, \"unreachable\")\n}\n",
    );
    let lowered = resolve_and_lower_package_for_service(&target).expect("boxed service lower");
    validate_http_service_handler(&lowered).expect("valid handler shape");
    assert!(
        lowered.rust.contains("checkpoint().await"),
        "service loop must remain cooperatively cancellable:\n{}",
        lowered.rust
    );
    assert!(
        lowered.rust.contains("pub fn call_export_with_host_until(")
            && lowered.rust.contains("block_on_until(deadline"),
        "service export needs the deadline seam:\n{}",
        lowered.rust
    );
}
