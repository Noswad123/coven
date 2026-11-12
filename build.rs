fn main() {
    const ACL_SOURCE: &str = "src/hook_files/platform/macos_acl.c";
    println!("cargo:rerun-if-changed={ACL_SOURCE}");
    // Build scripts run on the host: use Cargo's target, not the host's cfg.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        cc::Build::new()
            .file(ACL_SOURCE)
            .flag_if_supported("-std=c11")
            .warnings(true)
            .extra_warnings(true)
            // Never define COVEN_HOOK_ACL_TEST here: only SDK declarations are
            // acceptable for a native binary's authorization checks.
            .compile("coven_hook_acl");
    }
}
