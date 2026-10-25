fn main() {
    let attributes = tauri_build::Attributes::new();
    #[cfg(all(target_os = "windows", target_env = "msvc"))]
    let attributes = {
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!(
            "cargo:rustc-link-arg=/MANIFESTDEPENDENCY:type='win32' name='Microsoft.Windows.Common-Controls' version='6.0.0.0' processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'"
        );
        // The linker embeds the manifest above; keep Tauri's other resources
        // without adding a second RT_MANIFEST with the same resource ID.
        attributes.windows_attributes(tauri_build::WindowsAttributes::new_without_app_manifest())
    };
    tauri_build::try_build(attributes).expect("failed to run Tauri build script");
}
