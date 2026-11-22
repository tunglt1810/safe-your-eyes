//! Compiles `assets/icons/hicolor/64x64/apps/safe-your-eyes.png` into a compact RLE-encoded RGBA
//! table included directly by `sye-ui` (zero runtime image-decoding dependencies).

use std::env;
use std::error::Error;
use std::fmt::Write as _;
use std::fs::{self, File};
use std::io::BufReader;
use std::path::{Path, PathBuf};

const EXPECTED_SIZE: u32 = 64;

fn main() -> Result<(), Box<dyn Error>> {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?);
    let icon_path = manifest_dir
        .join("..")
        .join("..")
        .join("assets")
        .join("icons")
        .join("hicolor")
        .join("64x64")
        .join("apps")
        .join("safe-your-eyes.png");
    println!("cargo:rerun-if-changed={}", icon_path.display());

    let rgba = decode_rgba_png(&icon_path)?;
    let rle = encode_rle(&rgba);

    let mut generated = String::new();
    writeln!(generated, "const APP_ICON_SIZE: u32 = {EXPECTED_SIZE};")?;
    writeln!(generated, "const APP_ICON_RLE: &[u8] = &[")?;
    for chunk in rle.chunks(20) {
        generated.push_str("    ");
        for byte in chunk {
            write!(generated, "{byte}, ")?;
        }
        generated.push('\n');
    }
    writeln!(generated, "];")?;

    let out_dir = PathBuf::from(env::var("OUT_DIR")?);
    fs::write(out_dir.join("app_icon.rs"), generated)?;
    Ok(())
}

fn decode_rgba_png(path: &Path) -> Result<Vec<u8>, Box<dyn Error>> {
    let file = File::open(path)?;
    let mut decoder = png::Decoder::new(BufReader::new(file));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info()?;
    let mut buf = vec![
        0u8;
        reader
            .output_buffer_size()
            .ok_or("png output size overflow")?
    ];
    let info = reader.next_frame(&mut buf)?;
    buf.truncate(info.buffer_size());

    if info.width != EXPECTED_SIZE || info.height != EXPECTED_SIZE {
        return Err(format!(
            "{}: expected {EXPECTED_SIZE}x{EXPECTED_SIZE}, got {}x{}",
            path.display(),
            info.width,
            info.height
        )
        .into());
    }

    match info.color_type {
        png::ColorType::Rgba => Ok(buf),
        png::ColorType::Rgb => {
            let mut rgba = Vec::with_capacity((EXPECTED_SIZE * EXPECTED_SIZE * 4) as usize);
            for &[r, g, b] in buf.as_chunks::<3>().0 {
                rgba.extend_from_slice(&[r, g, b, 255]);
            }
            Ok(rgba)
        }
        other => Err(format!("{}: unsupported PNG color type {other:?}", path.display()).into()),
    }
}

fn encode_rle(rgba: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut iter = rgba.as_chunks::<4>().0.iter().copied();
    let Some(mut current) = iter.next() else {
        return out;
    };

    let mut count: u8 = 1;
    for next in iter {
        if next == current && count < u8::MAX {
            count += 1;
        } else {
            out.push(count);
            out.extend_from_slice(&current);
            current = next;
            count = 1;
        }
    }

    out.push(count);
    out.extend_from_slice(&current);
    out
}
