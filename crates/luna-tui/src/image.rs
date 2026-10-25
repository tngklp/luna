//! Image attachments.
//!
//! A terminal cannot show an image, so the TUI's job is to read one from disk,
//! check it is something a model will accept, and hand the bytes to the agent.
//! The transcript shows a placeholder instead.

use std::path::{Path, PathBuf};

use luna_core::types::ContentBlock;

/// Formats every provider accepts.
const SUPPORTED: &[(&str, &str)] = &[
    ("png", "image/png"),
    ("jpg", "image/jpeg"),
    ("jpeg", "image/jpeg"),
    ("gif", "image/gif"),
    ("webp", "image/webp"),
];

/// Images larger than this are refused: providers reject them, and base64 makes
/// them a third larger again.
pub const MAX_BYTES: u64 = 5 * 1024 * 1024;

/// One attached image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attachment {
    pub path: PathBuf,
    pub mime_type: String,
    /// Base64-encoded bytes.
    pub data: String,
    /// Size on disk, for the transcript.
    pub bytes: u64,
}

impl Attachment {
    /// The name shown in the transcript.
    pub fn name(&self) -> String {
        self.path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| self.path.display().to_string())
    }

    /// A one-line description, e.g. `photo.png (312 KB)`.
    pub fn label(&self) -> String {
        format!("{} ({})", self.name(), human_size(self.bytes))
    }

    /// The content block the provider encoders understand.
    pub fn to_block(&self) -> ContentBlock {
        ContentBlock::Image { mime_type: self.mime_type.clone(), data: self.data.clone() }
    }
}

/// Format a byte count for a human.
pub fn human_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{} KB", bytes / KB)
    } else {
        format!("{bytes} B")
    }
}

/// The MIME type for a path, from its extension.
pub fn mime_for(path: &Path) -> Option<&'static str> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    SUPPORTED.iter().find(|(ext, _)| *ext == extension).map(|(_, mime)| *mime)
}

/// Whether a path looks like an image Luna can attach.
pub fn is_image(path: &Path) -> bool {
    mime_for(path).is_some()
}

/// Check that the bytes really are the image the extension claims.
///
/// A corrupt file is rejected by providers with a generic "invalid request",
/// which sends the user hunting through their config. Checking the signature
/// here turns that into a message that names the file.
pub fn validate(bytes: &[u8], mime_type: &str) -> Result<(), String> {
    let ok = match mime_type {
        "image/png" => bytes.starts_with(&[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
        "image/jpeg" => bytes.starts_with(&[0xff, 0xd8, 0xff]),
        "image/gif" => bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a"),
        "image/webp" => bytes.len() > 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP",
        _ => false,
    };
    if ok {
        Ok(())
    } else {
        Err(format!("the file is not a valid {mime_type}"))
    }
}

/// Read an image from disk.
pub fn load(path: &Path) -> Result<Attachment, String> {
    let mime_type = mime_for(path)
        .ok_or_else(|| format!("{} is not a PNG, JPEG, GIF or WebP", path.display()))?;

    let metadata = std::fs::metadata(path)
        .map_err(|err| format!("could not read {}: {err}", path.display()))?;
    if metadata.len() > MAX_BYTES {
        return Err(format!(
            "{} is {}; the limit is {}",
            path.display(),
            human_size(metadata.len()),
            human_size(MAX_BYTES)
        ));
    }

    let bytes =
        std::fs::read(path).map_err(|err| format!("could not read {}: {err}", path.display()))?;
    validate(&bytes, mime_type).map_err(|err| format!("{}: {err}", path.display()))?;

    Ok(Attachment {
        path: path.to_path_buf(),
        mime_type: mime_type.to_string(),
        data: base64_encode(&bytes),
        bytes: metadata.len(),
    })
}

/// Standard base64, with padding.
pub fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let triple = (b0 << 16) | (b1 << 8) | b2;

        out.push(ALPHABET[(triple >> 18) as usize & 0x3f] as char);
        out.push(ALPHABET[(triple >> 12) as usize & 0x3f] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(triple >> 6) as usize & 0x3f] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 { ALPHABET[triple as usize & 0x3f] as char } else { '=' });
    }
    out
}

/// Pull image paths out of a message.
///
/// A path is only treated as an attachment when it exists and has an image
/// extension, so ordinary prose that happens to mention `logo.png` is left alone.
pub fn extract_paths(text: &str) -> Vec<PathBuf> {
    text.split_whitespace()
        .map(|word| word.trim_matches(|c: char| c == '"' || c == '\'' || c == ',' || c == ';'))
        .filter(|word| !word.is_empty())
        .map(PathBuf::from)
        .filter(|path| is_image(path) && path.exists())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("luna-img-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn base64_matches_the_standard_encoding() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn base64_handles_binary() {
        // A 1x1 PNG's first bytes.
        let bytes = [0x89, 0x50, 0x4e, 0x47];
        assert_eq!(base64_encode(&bytes), "iVBORw==");
    }

    #[test]
    fn mime_types_come_from_the_extension() {
        assert_eq!(mime_for(Path::new("a.png")), Some("image/png"));
        assert_eq!(mime_for(Path::new("a.JPG")), Some("image/jpeg"));
        assert_eq!(mime_for(Path::new("a.jpeg")), Some("image/jpeg"));
        assert_eq!(mime_for(Path::new("a.webp")), Some("image/webp"));
        assert_eq!(mime_for(Path::new("a.txt")), None);
        assert_eq!(mime_for(Path::new("a")), None);
    }

    #[test]
    fn sizes_are_human_readable() {
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(2048), "2 KB");
        assert_eq!(human_size(3 * 1024 * 1024), "3.0 MB");
    }

    #[test]
    fn an_image_round_trips_from_disk() {
        let dir = temp("load");
        let path = dir.join("dot.png");
        // A real PNG signature, so validation passes.
        std::fs::write(&path, [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]).unwrap();

        let attachment = load(&path).unwrap();
        assert_eq!(attachment.mime_type, "image/png");
        assert_eq!(attachment.data, "iVBORw0KGgo=");
        assert_eq!(attachment.name(), "dot.png");
        assert!(attachment.label().contains("dot.png"));
        assert!(matches!(attachment.to_block(), ContentBlock::Image { .. }));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_file_that_is_not_really_an_image_is_refused() {
        let dir = temp("corrupt");
        let path = dir.join("fake.png");
        std::fs::write(&path, b"this is not a png").unwrap();
        let error = load(&path).unwrap_err();
        assert!(error.contains("not a valid image/png"), "{error}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn signatures_are_checked_per_format() {
        assert!(validate(&[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a], "image/png").is_ok());
        assert!(validate(&[0xff, 0xd8, 0xff, 0xe0], "image/jpeg").is_ok());
        assert!(validate(b"GIF89a....", "image/gif").is_ok());
        assert!(validate(b"RIFF\0\0\0\0WEBPVP8 ", "image/webp").is_ok());
        assert!(validate(b"nope", "image/png").is_err());
        assert!(validate(b"nope", "image/jpeg").is_err());
    }

    #[test]
    fn a_non_image_is_refused() {
        let dir = temp("refuse");
        let path = dir.join("notes.txt");
        std::fs::write(&path, "hello").unwrap();
        assert!(load(&path).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_missing_file_is_refused() {
        assert!(load(Path::new("/nonexistent/x.png")).is_err());
    }

    #[test]
    fn paths_are_extracted_from_a_message() {
        let dir = temp("extract");
        let path = dir.join("shot.png");
        std::fs::write(&path, [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]).unwrap();

        let text = format!("look at \"{}\" please", path.display());
        let found = extract_paths(&text);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0], path);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn prose_mentioning_an_image_is_not_an_attachment() {
        assert!(extract_paths("the logo.png file is missing").is_empty());
    }

    #[test]
    fn a_non_image_path_is_not_extracted() {
        let dir = temp("nonimage");
        let path = dir.join("a.txt");
        std::fs::write(&path, "x").unwrap();
        assert!(extract_paths(&path.display().to_string()).is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }
}
