#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let theseus_dir = infernum_launcher_lib::data_root().join("theseus");
    unsafe { std::env::set_var("THESEUS_CONFIG_DIR", theseus_dir) };
    for var in [
        "MOD_CLASSES",
        "JAVA_TOOL_OPTIONS",
        "_JAVA_OPTIONS",
        "JDK_JAVA_OPTIONS",
    ] {
        unsafe { std::env::remove_var(var) };
    }

    infernum_launcher_lib::run()
}
