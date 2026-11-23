//! Embeds the multi-resolution Windows `.ico` resource into `safe-your-eyes.exe`.

fn main() {
    #[cfg(windows)]
    if let Err(err) = embed_windows_icon() {
        eprintln!("failed to embed Windows icon resource: {err}");
        std::process::exit(1);
    }
}

#[cfg(windows)]
fn embed_windows_icon() -> Result<(), Box<dyn std::error::Error>> {
    let icon_path = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")?)
        .join("..")
        .join("..")
        .join("assets")
        .join("icons")
        .join("app-icon.ico");
    println!("cargo::rerun-if-changed={}", icon_path.display());

    let mut res = winresource::WindowsResource::new();
    res.set_icon(&icon_path.to_string_lossy());
    res.compile()?;
    Ok(())
}
