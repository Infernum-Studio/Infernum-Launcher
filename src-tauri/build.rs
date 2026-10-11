fn main() {
    for var in ["INFERNUM_CDN_BASE_URL", "INFERNUM_MSA_ENABLED"] {
        println!("cargo::rerun-if-env-changed={var}");
    }
    tauri_build::build()
}
