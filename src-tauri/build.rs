use std::{env, path::PathBuf};

fn main() {
    let mut attributes = tauri_build::Attributes::new();

    // tauri-build embeds its Windows manifest (which asks for Common Controls
    // v6) only into the app binary. Test binaries then load Common Controls
    // v5 and crash at startup with STATUS_ENTRYPOINT_NOT_FOUND before any test
    // runs. Instead, embed the same manifest through the linker for every
    // target, and turn off tauri-build's copy so it isn't embedded twice.
    let windows_msvc = env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");
    if windows_msvc {
        let manifest =
            PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("windows-app-manifest.xml");
        println!("cargo:rerun-if-changed=windows-app-manifest.xml");
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
        attributes = attributes
            .windows_attributes(tauri_build::WindowsAttributes::new_without_app_manifest());
    }

    tauri_build::try_build(attributes).expect("failed to run tauri-build");
}
