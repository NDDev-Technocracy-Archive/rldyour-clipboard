//! Pure conversions at the Windows clipboard boundary.
#![forbid(unsafe_code)]

pub(super) const BMP_PREFIX: usize = 14;

pub(super) fn utf16_to_utf8(bytes: Vec<u8>) -> Vec<u8> {
    let units = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes(*pair))
        .take_while(|unit| *unit != 0);
    char::decode_utf16(units)
        .map(|character| character.unwrap_or(char::REPLACEMENT_CHARACTER))
        .collect::<String>()
        .into_bytes()
}

pub(super) fn strip_cf_html(bytes: Vec<u8>) -> Vec<u8> {
    let text = String::from_utf8_lossy(&bytes);
    let offset = |key: &str| -> Option<usize> {
        text.lines().find(|line| line.starts_with(key))?[key.len()..]
            .trim()
            .parse()
            .ok()
    };
    match (offset("StartFragment:"), offset("EndFragment:")) {
        (Some(start), Some(end)) if start < end && end <= bytes.len() => bytes[start..end].to_vec(),
        _ => bytes,
    }
}

/// Finish a BMP header in the space reserved before a native packed DIB.
/// BITMAPV4/V5 already contain their bit masks; only a 40-byte header needs
/// separate BI_BITFIELDS masks. Reject malformed dimensions, palette and rows.
pub(super) fn finish_bmp(bytes: &mut [u8]) -> Option<()> {
    let dib = bytes.get(BMP_PREFIX..)?;
    if dib.len() < 40 {
        return None;
    }
    let word = |at| u32::from_le_bytes(dib[at..at + 4].try_into().unwrap());
    let header = word(0) as usize;
    let width = word(4) as i32;
    let height = word(8) as i32;
    let planes = u16::from_le_bytes(dib[12..14].try_into().unwrap());
    let bits = u16::from_le_bytes(dib[14..16].try_into().unwrap());
    let compression = word(16);
    let colors = word(32) as usize;
    if !matches!(header, 40 | 52 | 56 | 108 | 124)
        || header > dib.len()
        || width <= 0
        || height == 0
        || height == i32::MIN
        || planes != 1
        || !matches!(bits, 1 | 4 | 8 | 16 | 24 | 32)
        || !matches!(compression, 0 | 3)
        || compression == 3 && !matches!(bits, 16 | 32)
    {
        return None;
    }
    let palette = if bits <= 8 {
        let maximum = 1usize << bits;
        if colors > maximum {
            return None;
        }
        if colors == 0 { maximum } else { colors }
    } else {
        colors
    };
    let masks = if header == 40 && compression == 3 {
        12
    } else {
        0
    };
    let pixels = BMP_PREFIX
        .checked_add(header)?
        .checked_add(masks)?
        .checked_add(palette.checked_mul(4)?)?;
    let row = (width as u64 * bits as u64).div_ceil(32).checked_mul(4)?;
    let pixel_bytes = row.checked_mul(height.unsigned_abs() as u64)?;
    if pixel_bytes > bytes.len().checked_sub(pixels)? as u64 {
        return None;
    }
    let total = u32::try_from(bytes.len()).ok()?;
    let pixels = u32::try_from(pixels).ok()?;
    bytes[..BMP_PREFIX].fill(0);
    bytes[..2].copy_from_slice(b"BM");
    bytes[2..6].copy_from_slice(&total.to_le_bytes());
    bytes[10..14].copy_from_slice(&pixels.to_le_bytes());
    Some(())
}

pub(super) fn file_uris(paths: &[std::path::PathBuf]) -> Option<Vec<u8>> {
    let mut result = String::new();
    for path in paths {
        // Do not manufacture a different filename with lossy conversion.
        let path = path.to_str()?.replace('\\', "/");
        let path = if let Some(unc) = path.strip_prefix("//?/UNC/") {
            format!("//{unc}")
        } else if let Some(drive) = path.strip_prefix("//?/") {
            drive.to_owned()
        } else {
            path
        };
        let prefix = if let Some(unc) = path.strip_prefix("//") {
            let (server, share) = unc.split_once('/')?;
            if server.is_empty() || server == "." || share.is_empty() {
                return None;
            }
            "file:"
        } else if path.len() >= 3
            && path.as_bytes()[0].is_ascii_alphabetic()
            && &path.as_bytes()[1..3] == b":/"
        {
            "file:///"
        } else {
            return None;
        };
        result.push_str(prefix);
        for byte in path.bytes() {
            if byte.is_ascii_alphanumeric() || b"-._~/:".contains(&byte) {
                result.push(byte as char);
            } else {
                use std::fmt::Write;
                write!(result, "%{byte:02X}").unwrap();
            }
        }
        result.push_str("\r\n");
    }
    Some(result.into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bitmap_headers_keep_v4_v5_masks_inside_their_existing_header() {
        for (header, extra_masks, offset) in [(40, 12, 66), (108, 0, 122), (124, 0, 138)] {
            let mut bitmap = vec![0; BMP_PREFIX + header + extra_masks + 4];
            bitmap[14..18].copy_from_slice(&(header as u32).to_le_bytes());
            bitmap[18..22].copy_from_slice(&1i32.to_le_bytes());
            bitmap[22..26].copy_from_slice(&(-1i32).to_le_bytes());
            bitmap[26..28].copy_from_slice(&1u16.to_le_bytes());
            bitmap[28..30].copy_from_slice(&32u16.to_le_bytes());
            bitmap[30..34].copy_from_slice(&3u32.to_le_bytes());
            let payload = bitmap[14..].to_vec();
            finish_bmp(&mut bitmap).unwrap();
            assert_eq!(&bitmap[..2], b"BM");
            assert_eq!(
                u32::from_le_bytes(bitmap[10..14].try_into().unwrap()),
                offset
            );
            assert_eq!(&bitmap[14..], payload);
            bitmap.pop();
            assert!(finish_bmp(&mut bitmap).is_none());
        }
    }

    #[test]
    fn malformed_bitmap_headers_never_publish_an_offset_outside_the_buffer() {
        for size in [0, 13, 14, 39, 53] {
            assert!(finish_bmp(&mut vec![0; size]).is_none());
        }
        let mut valid = vec![0; 58];
        valid[14..18].copy_from_slice(&40u32.to_le_bytes());
        valid[18..22].copy_from_slice(&1i32.to_le_bytes());
        valid[22..26].copy_from_slice(&1i32.to_le_bytes());
        valid[26..28].copy_from_slice(&1u16.to_le_bytes());
        valid[28..30].copy_from_slice(&24u16.to_le_bytes());
        finish_bmp(&mut valid).unwrap();
        for (field, value) in [
            (14, u32::MAX),
            (18, 0),
            (22, i32::MIN as u32),
            (46, u32::MAX),
        ] {
            let mut bad = valid.clone();
            bad[field..field + 4].copy_from_slice(&value.to_le_bytes());
            assert!(finish_bmp(&mut bad).is_none());
        }
    }

    #[test]
    fn file_urls_preserve_unicode_and_escaped_names_on_drive_unc_and_extended_paths() {
        let paths = [
            r"C:\photo #1%.png",
            r"\\server\share\二.txt",
            r"\\?\D:\a b.txt",
            r"\\?\UNC\host\share\q.txt",
        ];
        assert_eq!(
            file_uris(&paths.map(std::path::PathBuf::from)).unwrap(),
            b"file:///C:/photo%20%231%25.png\r\nfile://server/share/%E4%BA%8C.txt\r\nfile:///D:/a%20b.txt\r\nfile://host/share/q.txt\r\n"
        );
        for unsupported in ["relative", r"C:relative", r"\\.\device"] {
            assert!(file_uris(&[unsupported.into()]).is_none());
        }
        use std::os::windows::ffi::OsStringExt;
        let invalid = std::ffi::OsString::from_wide(&[0x43, 0x3a, 0x5c, 0xd800]);
        assert!(file_uris(&[invalid.into()]).is_none());
    }

    #[test]
    fn unicode_conversion_is_bounded_and_retains_surrogate_pairs() {
        let original = "русский қазақша العربية 中文 🖇";
        let bytes = original
            .encode_utf16()
            .chain([0, 0x61])
            .flat_map(u16::to_le_bytes)
            .collect();
        assert_eq!(utf16_to_utf8(bytes), original.as_bytes());
        assert_eq!(utf16_to_utf8(vec![0, 0xd8]), "�".as_bytes());
    }
}
