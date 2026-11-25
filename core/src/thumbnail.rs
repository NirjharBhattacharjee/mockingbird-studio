//! The thumbnail step's checks on `submit`: one PNG or JPEG, 1280x720,
//! under 2 MB. Only the headers are read, so there is no image library.

use crate::project::Result;
use std::{fs, io::Read, path::Path};

const MAX_BYTES: u64 = 2 * 1024 * 1024;
const SIZE: (u32, u32) = (1280, 720);

/// `files` are paths inside the project folder `dir`, already resolved.
pub fn check(dir: &Path, files: &[String]) -> Result<()> {
    let [file] = files else {
        return Err(format!(
            "the thumbnail step takes exactly one image, got {}",
            files.len()
        ));
    };
    let io = |e: std::io::Error| format!("{file}: {e}");
    let path = dir.join(file);
    let bytes = fs::metadata(&path).map_err(io)?.len();
    if bytes >= MAX_BYTES {
        return Err(format!(
            "{file} is {bytes} bytes; a thumbnail must be under 2 MB ({MAX_BYTES} bytes)"
        ));
    }
    // A JPEG's size can sit after other segments, so read the whole file (< 2 MB).
    let mut head = Vec::new();
    fs::File::open(&path)
        .and_then(|f| f.take(MAX_BYTES).read_to_end(&mut head))
        .map_err(io)?;
    let (kind, size) = dimensions(&head).ok_or_else(|| {
        format!("{file} is not a PNG or JPEG (or its header is damaged); a thumbnail must be one")
    })?;
    let ext = file.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase());
    let named = match ext.as_deref() {
        Some("png") => "PNG",
        Some("jpg" | "jpeg") => "JPEG",
        _ => return Err(format!("{file} must be named .png, .jpg or .jpeg")),
    };
    if named != kind {
        return Err(format!("{file} is named like a {named} but is a {kind}"));
    }
    if size != SIZE {
        return Err(format!(
            "{file} is {}x{} {kind}; a thumbnail must be {}x{}",
            size.0, size.1, SIZE.0, SIZE.1
        ));
    }
    Ok(())
}

/// The format and (width, height) from the file's own bytes, not its name.
fn dimensions(b: &[u8]) -> Option<(&'static str, (u32, u32))> {
    let be32 = |i: usize| {
        b.get(i..i + 4)
            .map(|x| u32::from_be_bytes(x.try_into().unwrap()))
    };
    let be16 = |i: usize| {
        b.get(i..i + 2)
            .map(|x| u16::from_be_bytes(x.try_into().unwrap()) as u32)
    };
    if b.starts_with(b"\x89PNG\r\n\x1a\n") && b.get(12..16)? == b"IHDR" {
        return Some(("PNG", (be32(16)?, be32(20)?)));
    }
    if !b.starts_with(&[0xFF, 0xD8]) {
        return None;
    }
    // Walk the segments to the first start-of-frame marker.
    let mut i = 2;
    loop {
        if *b.get(i)? != 0xFF {
            return None;
        }
        let marker = *b.get(i + 1)?;
        match marker {
            0xFF => i += 1, // fill byte
            0xC0..=0xCF if !matches!(marker, 0xC4 | 0xC8 | 0xCC) => {
                return Some(("JPEG", (be16(i + 7)?, be16(i + 5)?)));
            }
            0xD8 | 0x01 | 0xD0..=0xD7 => i += 2, // markers with no length
            0xD9 | 0xDA => return None,          // end or scan before any frame header
            _ => i += 2 + be16(i + 2)? as usize,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let mut v = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        v.extend(w.to_be_bytes());
        v.extend(h.to_be_bytes());
        v.extend([8, 2, 0, 0, 0]);
        v
    }

    fn jpeg(w: u16, h: u16) -> Vec<u8> {
        // SOI, an APP0 segment to step over, then SOF0.
        let mut v = vec![0xFF, 0xD8, 0xFF, 0xE0, 0, 4, 0, 0, 0xFF, 0xC0, 0, 11, 8];
        v.extend(h.to_be_bytes());
        v.extend(w.to_be_bytes());
        v.extend([1, 1, 0x11, 0]);
        v
    }

    fn try_file(name: &str, bytes: &[u8]) -> Result<()> {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(name), bytes).unwrap();
        check(dir.path(), &[name.to_string()])
    }

    #[test]
    fn right_size_png_and_jpeg_pass() {
        try_file("t.png", &png(1280, 720)).unwrap();
        try_file("t.jpg", &jpeg(1280, 720)).unwrap();
    }

    #[test]
    fn a_wrong_size_is_refused_with_the_size() {
        let e = try_file("t.png", &png(1920, 1080)).unwrap_err();
        assert!(e.contains("1920x1080") && e.contains("1280x720"), "{e}");
        let e = try_file("t.jpg", &jpeg(1280, 721)).unwrap_err();
        assert!(e.contains("1280x721"), "{e}");
    }

    #[test]
    fn a_wrong_type_is_refused_whatever_its_name() {
        let e = try_file("t.png", b"GIF89a....................").unwrap_err();
        assert!(e.contains("not a PNG or JPEG"), "{e}");
        assert!(try_file("t.jpg", &[0xFF, 0xD8, 0xFF, 0xDA]).is_err());
    }

    #[test]
    fn the_extension_must_match_the_type() {
        let e = try_file("t.jpg", &png(1280, 720)).unwrap_err();
        assert!(e.contains("named like a JPEG but is a PNG"), "{e}");
        let e = try_file("t.gif", &png(1280, 720)).unwrap_err();
        assert!(e.contains(".png, .jpg or .jpeg"), "{e}");
        try_file("t.JPEG", &jpeg(1280, 720)).unwrap();
    }

    #[test]
    fn an_oversized_file_is_refused() {
        let mut big = png(1280, 720);
        big.resize(MAX_BYTES as usize, 0);
        let e = try_file("t.png", &big).unwrap_err();
        assert!(e.contains("under 2 MB"), "{e}");
    }

    #[test]
    fn it_takes_exactly_one_file() {
        let dir = tempfile::tempdir().unwrap();
        assert!(check(dir.path(), &[]).is_err());
        assert!(check(dir.path(), &["a.png".into(), "b.png".into()]).is_err());
    }
}
