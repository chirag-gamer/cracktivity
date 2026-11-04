fn main() {
    // MSVC only: the GNU linker has no /MANIFEST args (and no such need).
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")
    {
        // tao imports comctl32 v6-only exports (SetWindowSubclass,
        // TaskDialogIndirect); a binary without a manifest binds comctl32 5.82
        // and dies at load with STATUS_ENTRYPOINT_NOT_FOUND. tauri-build embeds
        // its manifest into bins only, so the test harnesses would be killed at
        // startup. Instead the manifest is embedded by the linker into every
        // linked product (bin + test harnesses) — one embed per binary.
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("app.manifest");
        println!("cargo:rerun-if-changed={}", manifest.display());
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
        let windows = tauri_build::WindowsAttributes::new_without_app_manifest();
        tauri_build::try_build(tauri_build::Attributes::new().windows_attributes(windows))
            .expect("failed to run tauri-build");
    } else {
        tauri_build::build();
    }
}
