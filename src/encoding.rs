//! Encoding detection and conversion.
//!
//! Legacy Japanese encodings are converted with the Windows API so that no
//! conversion tables are linked into the executable.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Encoding {
    Utf8,
    Utf16Le,
    Utf16Be,
    Cp932,
    EucJp,
    Iso2022Jp,
}

impl Encoding {
    pub fn name(self) -> &'static str {
        match self {
            Encoding::Utf8 => "utf-8",
            Encoding::Utf16Le => "utf-16le",
            Encoding::Utf16Be => "utf-16be",
            Encoding::Cp932 => "cp932",
            Encoding::EucJp => "euc-jp",
            Encoding::Iso2022Jp => "iso-2022-jp",
        }
    }

    pub fn from_name(name: &str) -> Option<Encoding> {
        let name = name.to_ascii_lowercase();
        Some(match name.as_str() {
            "utf-8" | "utf8" => Encoding::Utf8,
            "utf-16le" => Encoding::Utf16Le,
            "utf-16be" => Encoding::Utf16Be,
            "cp932" | "shift_jis" | "sjis" => Encoding::Cp932,
            "euc-jp" => Encoding::EucJp,
            "iso-2022-jp" => Encoding::Iso2022Jp,
            _ => return None,
        })
    }

    /// True for the legacy Japanese encodings, in which East Asian ambiguous
    /// characters are double-byte and usually displayed as full width.
    pub fn is_legacy_japanese(self) -> bool {
        matches!(
            self,
            Encoding::Cp932 | Encoding::EucJp | Encoding::Iso2022Jp
        )
    }

    fn bom(self) -> &'static [u8] {
        match self {
            Encoding::Utf8 => b"\xEF\xBB\xBF",
            Encoding::Utf16Le => b"\xFF\xFE",
            Encoding::Utf16Be => b"\xFE\xFF",
            _ => b"",
        }
    }

    fn codepage(self) -> u32 {
        match self {
            Encoding::Cp932 => 932,
            Encoding::EucJp => 20932,
            Encoding::Iso2022Jp => 50220,
            _ => unreachable!("not a Windows code page encoding"),
        }
    }
}

#[derive(Debug)]
pub struct Decoded {
    pub text: String,
    pub encoding: Encoding,
    pub bom: bool,
}

/// Decodes `bytes`, detecting the encoding unless `forced` is given.
pub fn decode(bytes: &[u8], forced: Option<Encoding>) -> Result<Decoded, String> {
    let (encoding, bom) = match forced {
        Some(e) => (e, !e.bom().is_empty() && bytes.starts_with(e.bom())),
        None => detect(bytes),
    };
    let body = if bom {
        &bytes[encoding.bom().len()..]
    } else {
        bytes
    };
    let text = match encoding {
        Encoding::Utf8 => String::from_utf8(body.to_vec()).ok(),
        Encoding::Utf16Le | Encoding::Utf16Be => decode_utf16(body, encoding == Encoding::Utf16Be),
        _ => win::decode(body, encoding.codepage()),
    };
    let text = text.ok_or_else(|| {
        format!(
            "input is not valid {}; set 'encoding' in the config file",
            encoding.name()
        )
    })?;
    Ok(Decoded {
        text,
        encoding,
        bom,
    })
}

/// Encodes `text`, prepending the byte order mark if `bom` is set.
pub fn encode(text: &str, encoding: Encoding, bom: bool) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    if bom {
        out.extend_from_slice(encoding.bom());
    }
    match encoding {
        Encoding::Utf8 => out.extend_from_slice(text.as_bytes()),
        Encoding::Utf16Le => text
            .encode_utf16()
            .for_each(|u| out.extend(u.to_le_bytes())),
        Encoding::Utf16Be => text
            .encode_utf16()
            .for_each(|u| out.extend(u.to_be_bytes())),
        _ => out.extend(
            win::encode(text, encoding.codepage())
                .ok_or_else(|| format!("cannot convert output to {}", encoding.name()))?,
        ),
    }
    Ok(out)
}

fn decode_utf16(bytes: &[u8], big_endian: bool) -> Option<String> {
    if !bytes.len().is_multiple_of(2) {
        return None;
    }
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| {
            if big_endian {
                u16::from_be_bytes([b[0], b[1]])
            } else {
                u16::from_le_bytes([b[0], b[1]])
            }
        })
        .collect();
    String::from_utf16(&units).ok()
}

/// Returns the detected encoding and whether the input starts with a BOM.
pub fn detect(bytes: &[u8]) -> (Encoding, bool) {
    for e in [Encoding::Utf8, Encoding::Utf16Le, Encoding::Utf16Be] {
        if bytes.starts_with(e.bom()) {
            return (e, true);
        }
    }
    if bytes.is_ascii() && bytes.windows(2).any(|w| w == b"\x1B$") {
        return (Encoding::Iso2022Jp, false);
    }
    if std::str::from_utf8(bytes).is_ok() {
        return (Encoding::Utf8, false);
    }
    let e = match (score_sjis(bytes), score_eucjp(bytes)) {
        (None, Some(_)) => Encoding::EucJp,
        (Some(s), Some(e)) if e > s => Encoding::EucJp,
        _ => Encoding::Cp932,
    };
    (e, false)
}

/// Returns `None` if `bytes` is not valid CP932, otherwise the number of
/// kana characters (a hint for telling CP932 from EUC-JP).
fn score_sjis(bytes: &[u8]) -> Option<usize> {
    let (mut i, mut score) = (0, 0);
    while i < bytes.len() {
        match bytes[i] {
            0x00..=0x7F | 0xA1..=0xDF => i += 1,
            b @ (0x81..=0x9F | 0xE0..=0xFC) => {
                match bytes.get(i + 1) {
                    Some(0x40..=0x7E | 0x80..=0xFC) => {}
                    _ => return None,
                }
                if b == 0x82 || b == 0x83 {
                    score += 1;
                }
                i += 2;
            }
            _ => return None,
        }
    }
    Some(score)
}

/// Same as [`score_sjis`] for EUC-JP.
fn score_eucjp(bytes: &[u8]) -> Option<usize> {
    let is_trail = |b: Option<&u8>| matches!(b, Some(0xA1..=0xFE));
    let (mut i, mut score) = (0, 0);
    while i < bytes.len() {
        match bytes[i] {
            0x00..=0x7F => i += 1,
            0x8E if matches!(bytes.get(i + 1), Some(0xA1..=0xDF)) => i += 2,
            0x8F if is_trail(bytes.get(i + 1)) && is_trail(bytes.get(i + 2)) => i += 3,
            b @ 0xA1..=0xFE if is_trail(bytes.get(i + 1)) => {
                if b == 0xA4 || b == 0xA5 {
                    score += 1;
                }
                i += 2;
            }
            _ => return None,
        }
    }
    Some(score)
}

mod win {
    use windows_sys::Win32::Globalization::{
        MB_ERR_INVALID_CHARS, MultiByteToWideChar, WideCharToMultiByte,
    };

    pub fn decode(bytes: &[u8], codepage: u32) -> Option<String> {
        if bytes.is_empty() {
            return Some(String::new());
        }
        let len = i32::try_from(bytes.len()).ok()?;
        // ISO-2022-JP (50220) does not accept any flags.
        let flags = if codepage == 50220 {
            0
        } else {
            MB_ERR_INVALID_CHARS
        };
        // SAFETY: the pointers and lengths describe valid buffers.
        unsafe {
            let n = MultiByteToWideChar(
                codepage,
                flags,
                bytes.as_ptr(),
                len,
                std::ptr::null_mut(),
                0,
            );
            if n <= 0 {
                return None;
            }
            let mut buf = vec![0u16; n as usize];
            let n = MultiByteToWideChar(codepage, flags, bytes.as_ptr(), len, buf.as_mut_ptr(), n);
            if n <= 0 {
                return None;
            }
            buf.truncate(n as usize);
            String::from_utf16(&buf).ok()
        }
    }

    pub fn encode(text: &str, codepage: u32) -> Option<Vec<u8>> {
        if text.is_empty() {
            return Some(Vec::new());
        }
        let wide: Vec<u16> = text.encode_utf16().collect();
        let len = i32::try_from(wide.len()).ok()?;
        // SAFETY: the pointers and lengths describe valid buffers.
        unsafe {
            let n = WideCharToMultiByte(
                codepage,
                0,
                wide.as_ptr(),
                len,
                std::ptr::null_mut(),
                0,
                std::ptr::null(),
                std::ptr::null_mut(),
            );
            if n <= 0 {
                return None;
            }
            let mut buf = vec![0u8; n as usize];
            let n = WideCharToMultiByte(
                codepage,
                0,
                wide.as_ptr(),
                len,
                buf.as_mut_ptr(),
                n,
                std::ptr::null(),
                std::ptr::null_mut(),
            );
            if n <= 0 {
                return None;
            }
            buf.truncate(n as usize);
            Some(buf)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = "吾輩は猫である。名前はまだ無い。\r\nWhat?\r\n";

    #[test]
    fn detects_cp932_with_halfwidth_kana() {
        let bytes = encode("ｶﾀｶﾅとかな①", Encoding::Cp932, false).unwrap();
        assert_eq!(detect(&bytes), (Encoding::Cp932, false));
    }

    #[test]
    fn round_trips_all_encodings() {
        for e in [
            Encoding::Utf8,
            Encoding::Utf16Le,
            Encoding::Utf16Be,
            Encoding::Cp932,
            Encoding::EucJp,
            Encoding::Iso2022Jp,
        ] {
            let bytes = encode(TEXT, e, false).unwrap();
            let d = decode(&bytes, Some(e)).unwrap();
            assert_eq!(d.text, TEXT, "{}", e.name());
        }
    }

    #[test]
    fn detects_encodings() {
        for e in [
            Encoding::Utf8,
            Encoding::Cp932,
            Encoding::EucJp,
            Encoding::Iso2022Jp,
        ] {
            let bytes = encode(TEXT, e, false).unwrap();
            let d = decode(&bytes, None).unwrap();
            assert_eq!((d.encoding, d.bom), (e, false), "{}", e.name());
            assert_eq!(d.text, TEXT);
        }
        for e in [Encoding::Utf8, Encoding::Utf16Le, Encoding::Utf16Be] {
            let bytes = encode(TEXT, e, true).unwrap();
            let d = decode(&bytes, None).unwrap();
            assert_eq!((d.encoding, d.bom), (e, true), "{}", e.name());
            assert_eq!(d.text, TEXT);
        }
        assert_eq!(detect(b"plain ascii"), (Encoding::Utf8, false));
    }

    #[test]
    fn rejects_invalid_input() {
        assert!(decode(b"\x82", Some(Encoding::Cp932)).is_err());
        assert!(decode(b"\xFF\xFF", Some(Encoding::Utf8)).is_err());
    }
}
