fn main() {
    for var in ["INFERNUM_CDN_BASE_URL", "INFERNUM_MSA_ENABLED"] {
        println!("cargo::rerun-if-env-changed={var}");
    }

    // Incrustamos Common-Controls v6 en todos los binarios
    // Sin el manifest Windows carga comctl32 v5.
    let mut attrs = tauri_build::Attributes::new();
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        let manifest = std::env::current_dir()
            .unwrap()
            .join("windows-app-manifest.xml");
        println!("cargo::rerun-if-changed={}", manifest.display());
        println!("cargo::rustc-link-arg=/MANIFEST:EMBED");
        println!(
            "cargo::rustc-link-arg=/MANIFESTINPUT:{}",
            manifest.display()
        );
        attrs =
            attrs.windows_attributes(tauri_build::WindowsAttributes::new_without_app_manifest());
    }
    tauri_build::try_build(attrs).expect("failed to run tauri-build");
}
