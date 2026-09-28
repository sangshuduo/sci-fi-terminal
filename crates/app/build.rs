//! Embed the application icon into the Windows executable.
//!
//! On other platforms this does nothing: macOS takes its Dock icon from the
//! `.app` bundle's `.icns`, and Linux desktops use the `.desktop` entry's PNG.

fn main() {
    println!("cargo:rerun-if-changed=../../assets/icons/sci-fi-terminal.ico");
    #[cfg(windows)]
    windows::embed_icon();
}

#[cfg(windows)]
mod windows {
    use std::path::PathBuf;

    /// Generate the resource script with an absolute icon path, so it does not
    /// depend on how the resource compiler resolves relative paths.
    pub fn embed_icon() {
        let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("manifest dir"));
        let icon = manifest.join("../../assets/icons/sci-fi-terminal.ico");
        let icon = icon
            .canonicalize()
            .expect("assets/icons/sci-fi-terminal.ico exists");
        let escaped = icon.display().to_string().replace('\\', "\\\\");
        let out = PathBuf::from(std::env::var("OUT_DIR").expect("out dir")).join("app.rc");
        std::fs::write(&out, format!("1 ICON \"{escaped}\"\n")).expect("write app.rc");
        embed_resource::compile(&out, embed_resource::NONE)
            .manifest_optional()
            .expect("embed the application icon into the executable");
    }
}
