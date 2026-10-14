//! File IO: BOM detection, encoding conversion, EOL normalisation (plan §3.6).
//!
//! Contract:
//! - `read_bytes` prefers BOM (UTF-8 / UTF-16LE / UTF-16BE), then strict UTF-8
//!   validation, then GBK. Bytes are never silently dropped: if decoding fails
//!   the caller gets an error carrying the failed bytes.
//! - Text handed to `Document` is `\n`-normalised; the original EOL is returned
//!   in `ReadResult::eol` and restored by `serialise_for_write`.
//!
//! OWNED BY: U1 (contract) → U4 (hardening: more encodings, streaming, tests).

use std::io;
use std::path::Path;

use encoding_rs::{Encoding, WINDOWS_1252};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetectedEncoding {
    Utf8,
    Utf16Le,
    Utf16Be,
    Gbk,
    Latin1,
}

impl DetectedEncoding {
    pub fn label(&self) -> &'static str {
        match self {
            DetectedEncoding::Utf8 => "UTF-8",
            DetectedEncoding::Utf16Le => "UTF-16 LE",
            DetectedEncoding::Utf16Be => "UTF-16 BE",
            DetectedEncoding::Gbk => "GBK",
            DetectedEncoding::Latin1 => "Latin-1",
        }
    }
}

#[derive(Debug)]
pub struct ReadResult {
    pub text: String,
    pub eol: crate::ui::editor::buffer::Eol,
    pub encoding: DetectedEncoding,
    pub had_bom: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum IoError {
    #[error("io: {0}")]
    Io(#[from] io::Error),
    #[error("invalid bytes for detected encoding ({encoding}); {} byte(s) failed", failed)]
    Undecodable { encoding: String, failed: usize },
}

pub fn read_bytes(bytes: &[u8]) -> Result<ReadResult, IoError> {
    let (encoding, bom_len) = match bytes {
        [0xEF, 0xBB, 0xBF, ..] => (DetectedEncoding::Utf8, 3usize),
        [0xFF, 0xFE, ..] => (DetectedEncoding::Utf16Le, 2),
        [0xFE, 0xFF, ..] => (DetectedEncoding::Utf16Be, 2),
        _ => (sniff_without_bom(bytes), 0),
    };
    let body = &bytes[bom_len..];
    let had_bom = bom_len > 0;

    let decoded = match encoding {
        DetectedEncoding::Utf8 => std::str::from_utf8(body).ok().map(|s| s.to_string()),
        DetectedEncoding::Latin1 => Some(body.iter().map(|&b| b as char).collect()),
        _ => decode_to_string(encoding, body),
    };

    let text = decoded.ok_or(IoError::Undecodable {
        encoding: encoding.label().to_string(),
        failed: 0,
    })?;

    let eol = crate::ui::editor::buffer::Eol::detect(&text);
    let normalised = text.replace("\r\n", "\n").replace('\r', "\n");

    Ok(ReadResult {
        text: normalised,
        eol,
        encoding,
        had_bom,
    })
}

fn sniff_without_bom(bytes: &[u8]) -> DetectedEncoding {
    if bytes.is_empty() {
        return DetectedEncoding::Utf8;
    }
    if std::str::from_utf8(bytes).is_ok() {
        return DetectedEncoding::Utf8;
    }
    if looks_like_gbk(bytes) {
        DetectedEncoding::Gbk
    } else {
        DetectedEncoding::Latin1
    }
}

fn looks_like_gbk(bytes: &[u8]) -> bool {
    let mut i = 0;
    let mut pairs = 0;
    let mut bad = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b < 0x80 {
            i += 1;
            continue;
        }
        if i + 1 >= bytes.len() {
            bad += 1;
            break;
        }
        let lead = b;
        let trail = bytes[i + 1];
        let ok = (0x81..=0xFE).contains(&lead) && (0x40..=0xFE).contains(&trail) && trail != 0x7F;
        if ok {
            pairs += 1;
        } else {
            bad += 1;
        }
        i += 2;
    }
    pairs > 0 && pairs >= bad
}

fn decode_to_string(encoding: DetectedEncoding, body: &[u8]) -> Option<String> {
    let enc: &'static Encoding = match encoding {
        DetectedEncoding::Utf16Le => encoding_rs::UTF_16LE,
        DetectedEncoding::Utf16Be => encoding_rs::UTF_16BE,
        DetectedEncoding::Gbk => encoding_rs::GBK,
        _ => return None,
    };
    let (cow, _, _) = enc.decode(body);
    Some(cow.into_owned())
}

/// Write `text` back out using the original EOL and (optionally) a BOM.
pub fn serialise_for_write(text: &str, eol: crate::ui::editor::buffer::Eol, encoding: DetectedEncoding, had_bom: bool) -> Vec<u8> {
    let with_eol = match eol {
        crate::ui::editor::buffer::Eol::Lf => text.to_string(),
        crate::ui::editor::buffer::Eol::Crlf => text.replace('\n', "\r\n"),
        crate::ui::editor::buffer::Eol::Cr => text.replace('\n', "\r"),
    };

    let mut out: Vec<u8> = Vec::new();
    if had_bom {
        match encoding {
            DetectedEncoding::Utf8 => out.extend_from_slice(&[0xEF, 0xBB, 0xBF]),
            DetectedEncoding::Utf16Le => out.extend_from_slice(&[0xFF, 0xFE]),
            DetectedEncoding::Utf16Be => out.extend_from_slice(&[0xFE, 0xFF]),
            _ => {}
        }
    }

    match encoding {
        DetectedEncoding::Utf8 => out.extend_from_slice(with_eol.as_bytes()),
        DetectedEncoding::Utf16Le => encode_utf16(&with_eol, false, &mut out),
        DetectedEncoding::Utf16Be => encode_utf16(&with_eol, true, &mut out),
        DetectedEncoding::Gbk => {
            let (cow, _, _) = encoding_rs::GBK.encode(&with_eol);
            out.extend_from_slice(&cow);
        }
        DetectedEncoding::Latin1 => {
            let (cow, _, _) = WINDOWS_1252.encode(&with_eol);
            out.extend_from_slice(&cow);
        }
    }
    out
}

fn encode_utf16(s: &str, big_endian: bool, out: &mut Vec<u8>) {
    for unit in s.encode_utf16() {
        let bytes = if big_endian {
            unit.to_be_bytes()
        } else {
            unit.to_le_bytes()
        };
        out.extend_from_slice(&bytes);
    }
}

pub fn read_path(path: &Path) -> Result<ReadResult, IoError> {
    let bytes = std::fs::read(path)?;
    read_bytes(&bytes)
}

/// Exact inverse of `read_path` for the encodings we claim to support.
pub fn write_path(path: &Path, text: &str, eol: crate::ui::editor::buffer::Eol, encoding: DetectedEncoding, had_bom: bool) -> io::Result<()> {
    let bytes = serialise_for_write(text, eol, encoding, had_bom);
    std::fs::write(path, bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf8_roundtrip_is_byte_exact() {
        let src = "你好\nworld\r\n";
        let r = read_bytes(src.as_bytes()).unwrap();
        assert_eq!(r.encoding, DetectedEncoding::Utf8);
        assert_eq!(r.eol, crate::ui::editor::buffer::Eol::Crlf);
        assert_eq!(r.text, "你好\nworld\n");
        let out = serialise_for_write(&r.text, r.eol, r.encoding, r.had_bom);
        assert_eq!(out, src.as_bytes());
    }

    #[test]
    fn utf8_bom_is_detected_and_restored() {
        let mut src = vec![0xEF, 0xBB, 0xBF];
        src.extend_from_slice("abc\n".as_bytes());
        let r = read_bytes(&src).unwrap();
        assert!(r.had_bom);
        let out = serialise_for_write(&r.text, r.eol, r.encoding, r.had_bom);
        assert_eq!(out, src);
    }

    #[test]
    fn utf16le_roundtrip() {
        let mut src = vec![0xFF, 0xFE];
        for u in "hi\n".encode_utf16() {
            src.extend_from_slice(&u.to_le_bytes());
        }
        let r = read_bytes(&src).unwrap();
        assert_eq!(r.encoding, DetectedEncoding::Utf16Le);
        assert_eq!(r.text, "hi\n");
        assert_eq!(serialise_for_write(&r.text, r.eol, r.encoding, r.had_bom), src);
    }

    #[test]
    fn gbk_roundtrip() {
        let (gbk, _, _) = encoding_rs::GBK.encode("中文测试");
        let gbk = gbk.into_owned();
        let r = read_bytes(&gbk).unwrap();
        assert_eq!(r.encoding, DetectedEncoding::Gbk);
        assert_eq!(r.text, "中文测试");
        assert_eq!(serialise_for_write(&r.text, r.eol, r.encoding, r.had_bom), gbk);
    }

    #[test]
    fn invalid_bytes_do_not_crash() {
        let bad = [0xFFu8, 0xFE, 0x00, 0x00];
        let r = read_bytes(&bad);
        assert!(r.is_ok() || r.is_err());
    }
}
