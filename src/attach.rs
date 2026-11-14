//! Image attachments: clipboard / dropped files are normalized into
//! `%APPDATA%\Open Agent\attachments` so threads can show them later and the
//! agents get a format they accept (PNG, JPEG, GIF or WebP, under ~3.5 MB).

use image::{DynamicImage, ImageFormat};
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

/// Longest side we send; both agents downscale larger images anyway.
const MAX_SIDE: u32 = 2000;
/// Claude rejects images over 5 MB once base64-encoded (≈3.75 MB raw).
const MAX_BYTES: usize = 3_500_000;
const EXTENSIONS: [&str; 5] = ["png", "jpg", "jpeg", "gif", "webp"];

fn dir() -> PathBuf {
    crate::model::data_dir().join("attachments")
}

pub fn is_image_path(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

pub fn media_type(path: &str) -> &'static str {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match ext.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        _ => "image/png",
    }
}

fn store(bytes: &[u8], ext: &str) -> Result<String, String> {
    static SEQ: AtomicU32 = AtomicU32::new(0);
    let dir = dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("Can't create {}: {e}", dir.display()))?;
    let name = format!(
        "{}-{}.{ext}",
        crate::model::now_ms(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    );
    let path = dir.join(name);
    std::fs::write(&path, bytes).map_err(|e| format!("Can't save image: {e}"))?;
    Ok(path.to_string_lossy().into_owned())
}

/// Downscale and encode as PNG, falling back to JPEG when PNG is too large (photos).
fn encode(img: DynamicImage) -> Result<(Vec<u8>, &'static str), String> {
    let img = if img.width().max(img.height()) > MAX_SIDE {
        img.resize(MAX_SIDE, MAX_SIDE, image::imageops::FilterType::Lanczos3)
    } else {
        img
    };
    let mut png = Vec::new();
    img.write_to(&mut Cursor::new(&mut png), ImageFormat::Png)
        .map_err(|e| format!("Can't encode image: {e}"))?;
    if png.len() <= MAX_BYTES {
        return Ok((png, "png"));
    }
    let mut jpg = Vec::new();
    let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpg, 85);
    DynamicImage::ImageRgb8(img.to_rgb8())
        .write_with_encoder(enc)
        .map_err(|e| format!("Can't encode image: {e}"))?;
    Ok((jpg, "jpg"))
}

/// The image on the clipboard, saved as an attachment. `Ok(None)` when the clipboard holds no image.
pub fn from_clipboard() -> Result<Option<String>, String> {
    let mut cb = arboard::Clipboard::new().map_err(|e| format!("Clipboard unavailable: {e}"))?;
    let Ok(data) = cb.get_image() else {
        return Ok(None);
    };
    let rgba = image::RgbaImage::from_raw(
        data.width as u32,
        data.height as u32,
        data.bytes.into_owned(),
    )
    .ok_or("Clipboard image has an unexpected size")?;
    let (bytes, ext) = encode(DynamicImage::ImageRgba8(rgba))?;
    store(&bytes, ext).map(Some)
}

/// Files copied in Explorer (Ctrl+C on a file) arrive as a file list, not image data.
pub fn clipboard_files() -> Vec<PathBuf> {
    arboard::Clipboard::new()
        .ok()
        .and_then(|mut cb| cb.get().file_list().ok())
        .unwrap_or_default()
}

/// Copy an image file into the attachments folder, re-encoding it only when needed.
pub fn from_file(path: &Path) -> Result<String, String> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let bytes = std::fs::read(path).map_err(|e| format!("Can't read {name}: {e}"))?;
    let img =
        image::load_from_memory(&bytes).map_err(|_| format!("{name} isn't a supported image"))?;
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if EXTENSIONS.contains(&ext.as_str())
        && bytes.len() <= MAX_BYTES
        && img.width().max(img.height()) <= MAX_SIDE
    {
        return store(&bytes, &ext);
    }
    let (bytes, ext) = encode(img)?;
    store(&bytes, ext)
}

/// A small RGBA preview for the composer and the thread.
pub fn thumbnail(path: &str, px: u32) -> Option<image::RgbaImage> {
    let img = image::open(path).ok()?;
    Some(img.thumbnail(px, px).into_rgba8())
}

pub fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            T[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            T[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_rfc4648() {
        for (raw, enc) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(raw.as_bytes()), enc);
        }
    }

    #[test]
    fn media_types() {
        assert_eq!(media_type("a/b.JPG"), "image/jpeg");
        assert_eq!(media_type("x.webp"), "image/webp");
        assert_eq!(media_type("x.png"), "image/png");
        assert!(is_image_path(Path::new("C:/x/Shot.PNG")));
        assert!(!is_image_path(Path::new("notes.txt")));
    }
}
