//! Compiles PNG tray icon assets in `assets/icons/tray/` into compact RLE-encoded RGBA tables
//! included directly in the binary (zero runtime image-decoding dependencies).

use std::env;
use std::error::Error;
use std::fmt::Write as _;
use std::fs::{self, File};
use std::io::BufReader;
use std::path::{Path, PathBuf};

const EXPECTED_SIZE: u32 = 64;

const TRAY_ICONS: &[(&str, &str)] = &[
    ("COLOR_ENABLED_RLE", "tray-enabled.png"),
    ("COLOR_PRE_BREAK_RLE", "tray-pre-break.png"),
    ("COLOR_DISABLED_RLE", "tray-disabled.png"),
    ("TEMPLATE_ENABLED_RLE", "tray-enabled-template.png"),
    ("TEMPLATE_PRE_BREAK_RLE", "tray-pre-break-template.png"),
    ("TEMPLATE_DISABLED_RLE", "tray-disabled-template.png"),
];

fn main() -> Result<(), Box<dyn Error>> {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?);
    let tray_dir = manifest_dir
        .join("..")
        .join("..")
        .join("assets")
        .join("icons")
        .join("tray");
    let out_dir = PathBuf::from(env::var("OUT_DIR")?);

    let mut generated = String::new();
    writeln!(generated, "const ICON_SIZE: u32 = {EXPECTED_SIZE};")?;

    for (const_name, filename) in TRAY_ICONS {
        let path = tray_dir.join(filename);
        println!("cargo:rerun-if-changed={}", path.display());

        let rgba = decode_rgba_png(&path)?;
        let rle = encode_rle(&rgba);

        writeln!(generated, "const {const_name}: &[u8] = &[")?;
        for chunk in rle.chunks(20) {
            generated.push_str("    ");
            for byte in chunk {
                write!(generated, "{byte}, ")?;
            }
            generated.push('\n');
        }
        writeln!(generated, "];")?;
    }

    fs::write(out_dir.join("tray_icons.rs"), generated)?;
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

/// Run-length encodes RGBA pixels as repeating 5-byte records: `[count: 1..=255, r, g, b, a]`.
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
