fn main() {
    #[cfg(feature = "tauri")]
    tauri_plugin::Builder::new(&[
        "init",
        "track",
        "onboarding",
        "set_props",
        "install_id",
        "reset",
        "disable",
    ])
    .build();
}
