//! Embeds the multi-resolution Windows `.ico` resource into `safe-your-eyes.exe`.

use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    #[cfg(windows)]
    {
        let icon_path = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")?)
            .join("..")
            .join("..")
            .join("assets")
            .join("icons")
            .join("app-icon.ico");
        println!("cargo:rerun-if-changed={}", icon_path.display());

        let mut res = winresource::WindowsResource::new();
        res.set_icon(&icon_path.to_string_lossy());
        res.compile()?;
    }
    Ok(())
}
