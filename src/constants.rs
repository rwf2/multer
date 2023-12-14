use std::borrow::Cow;

pub(crate) const DEFAULT_WHOLE_STREAM_SIZE_LIMIT: u64 = std::u64::MAX;
pub(crate) const DEFAULT_PER_FIELD_SIZE_LIMIT: u64 = std::u64::MAX;

pub(crate) const MAX_HEADERS: usize = 32;
pub(crate) const BOUNDARY_EXT: &str = "--";
pub(crate) const CR: &str = "\r";
#[allow(dead_code)]
pub(crate) const LF: &str = "\n";
pub(crate) const CRLF: &str = "\r\n";
pub(crate) const CRLF_CRLF: &str = "\r\n\r\n";

#[derive(PartialEq)]
pub(crate) enum ContentDispositionAttr {
    Name,
    FileName,
}

fn trim_ascii_ws_start(bytes: &[u8]) -> &[u8] {
    bytes
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .map_or_else(|| &bytes[bytes.len()..], |i| &bytes[i..])
}

fn trim_ascii_ws_then(bytes: &[u8], char: u8) -> Option<&[u8]> {
    match trim_ascii_ws_start(bytes) {
        [first, rest @ ..] if *first == char => Some(rest),
        _ => None,
    }
}

impl ContentDispositionAttr {
    /// Extract ContentDisposition Attribute from header.
    ///
    /// Some older clients may not quote the name or filename, so we allow them,
    /// but require them to be percent encoded. Only allocates if percent
    /// decoding, and there are characters that need to be decoded.
    pub fn extract_from<'h>(&self, mut header: &'h [u8]) -> Option<Cow<'h, str>> {
        // TODO: The prefix should be matched case-insensitively.
        let prefix = match self {
            ContentDispositionAttr::Name => &b"name"[..],
            ContentDispositionAttr::FileName => &b"filename"[..],
        };

        while let Some(i) = memchr::memmem::find(header, prefix) {
            // Check if we found a superstring of `prefix`; continue if so.
            let suffix = &header[(i + prefix.len())..];
            if i > 0 && !(header[i - 1].is_ascii_whitespace() || header[i - 1] == b';') {
                header = suffix;
                continue;
            }

            // Now find and trim the `=`. Handle quoted strings first.
            let rest = trim_ascii_ws_then(suffix, b'=')?;
            let (bytes, is_escaped) = if let Some(rest) = trim_ascii_ws_then(rest, b'"') {
                let (mut k, mut escaped) = (memchr::memchr(b'"', rest)?, false);
                while k > 0 && rest[k - 1] == b'\\' {
                    escaped = true;
                    k = k + 1 + memchr::memchr(b'"', &rest[(k + 1)..])?;
                }

                (&rest[..k], escaped)
            } else {
                let rest = trim_ascii_ws_start(rest);
                let j = memchr::memchr2(b';', b' ', rest).unwrap_or(rest.len());
                (&rest[..j], false)
            };

            return match std::str::from_utf8(bytes).ok()? {
                name if is_escaped => Some(name.replace(r#"\""#, "\"").into()),
                name => Some(name.into()),
            };
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_content_disposition_name_only() {
        let val = br#"form-data; name="my_field""#;
        let name = ContentDispositionAttr::Name.extract_from(val);
        let filename = ContentDispositionAttr::FileName.extract_from(val);
        assert_eq!(name.unwrap(), "my_field");
        assert!(filename.is_none());

        let val = br#"form-data; name=my_field  "#;
        let name = ContentDispositionAttr::Name.extract_from(val);
        let filename = ContentDispositionAttr::FileName.extract_from(val);
        assert_eq!(name.unwrap(), "my_field");
        assert!(filename.is_none());

        let val = br#"form-data; name  =  my_field  "#;
        let name = ContentDispositionAttr::Name.extract_from(val);
        let filename = ContentDispositionAttr::FileName.extract_from(val);
        assert_eq!(name.unwrap(), "my_field");
        assert!(filename.is_none());

        let val = br#"form-data; name  =  "#;
        let name = ContentDispositionAttr::Name.extract_from(val);
        let filename = ContentDispositionAttr::FileName.extract_from(val);
        assert_eq!(name.unwrap(), "");
        assert!(filename.is_none());
    }

    #[test]
    fn test_content_disposition_extraction() {
        let val = br#"form-data; name="my_field"; filename="file abc.txt""#;
        let name = ContentDispositionAttr::Name.extract_from(val);
        let filename = ContentDispositionAttr::FileName.extract_from(val);
        assert_eq!(name.unwrap(), "my_field");
        assert_eq!(filename.unwrap(), "file abc.txt");

        let val = "form-data; name=\"你好\"; filename=\"file abc.txt\"".as_bytes();
        let name = ContentDispositionAttr::Name.extract_from(val);
        let filename = ContentDispositionAttr::FileName.extract_from(val);
        assert_eq!(name.unwrap(), "你好");
        assert_eq!(filename.unwrap(), "file abc.txt");

        let val = "form-data; name=\"কখগ\"; filename=\"你好.txt\"".as_bytes();
        let name = ContentDispositionAttr::Name.extract_from(val);
        let filename = ContentDispositionAttr::FileName.extract_from(val);
        assert_eq!(name.unwrap(), "কখগ");
        assert_eq!(filename.unwrap(), "你好.txt");
    }

    #[test]
    fn test_content_disposition_file_name_only() {
        // These are technically malformed, as RFC 7578 says the `name`
        // parameter _must_ be included. But okay.
        let val = br#"form-data; filename="file-name.txt""#;
        let name = ContentDispositionAttr::Name.extract_from(val);
        let filename = ContentDispositionAttr::FileName.extract_from(val);
        assert_eq!(filename.unwrap(), "file-name.txt");
        assert!(name.is_none());

        let val = "form-data; filename=\"কখগ-你好.txt\"".as_bytes();
        let name = ContentDispositionAttr::Name.extract_from(val);
        let filename = ContentDispositionAttr::FileName.extract_from(val);
        assert_eq!(filename.unwrap(), "কখগ-你好.txt");
        assert!(name.is_none());
    }

    #[test]
    fn test_content_distribution_misordered_fields() {
        let val = br#"form-data; filename=file-name.txt; name=file"#;
        let name = ContentDispositionAttr::Name.extract_from(val);
        let filename = ContentDispositionAttr::FileName.extract_from(val);
        assert_eq!(filename.unwrap(), "file-name.txt");
        assert_eq!(name.unwrap(), "file");

        let val = br#"form-data; filename="file-name.txt"; name="file""#;
        let name = ContentDispositionAttr::Name.extract_from(val);
        let filename = ContentDispositionAttr::FileName.extract_from(val);
        assert_eq!(filename.unwrap(), "file-name.txt");
        assert_eq!(name.unwrap(), "file");

        let val = "form-data; filename=\"你好.txt\"; name=\"কখগ\"".as_bytes();
        let name = ContentDispositionAttr::Name.extract_from(val);
        let filename = ContentDispositionAttr::FileName.extract_from(val);
        assert_eq!(name.unwrap(), "কখগ");
        assert_eq!(filename.unwrap(), "你好.txt");
    }

    #[test]
    fn test_content_disposition_name_unquoted() {
        let val = br#"form-data; name=my_field"#;
        let name = ContentDispositionAttr::Name.extract_from(val);
        let filename = ContentDispositionAttr::FileName.extract_from(val);
        assert_eq!(name.unwrap(), "my_field");
        assert!(filename.is_none());

        let val = br#"form-data; name=my_field; filename=file-name.txt"#;
        let name = ContentDispositionAttr::Name.extract_from(val);
        let filename = ContentDispositionAttr::FileName.extract_from(val);
        assert_eq!(name.unwrap(), "my_field");
        assert_eq!(filename.unwrap(), "file-name.txt");
    }

    #[test]
    fn test_content_disposition_name_quoted() {
        let val = br#"form-data; name="my;f;ield""#;
        let name = ContentDispositionAttr::Name.extract_from(val);
        let filename = ContentDispositionAttr::FileName.extract_from(val);
        assert_eq!(name.unwrap(), "my;f;ield");
        assert!(filename.is_none());

        let val = br#"form-data; name=my_field; filename = "file;name.txt""#;
        let name = ContentDispositionAttr::Name.extract_from(val);
        assert_eq!(name.unwrap(), "my_field");
        let filename = ContentDispositionAttr::FileName.extract_from(val);
        assert_eq!(filename.unwrap(), "file;name.txt");

        let val = br#"form-data; name=; filename=filename.txt"#;
        let name = ContentDispositionAttr::Name.extract_from(val);
        let filename = ContentDispositionAttr::FileName.extract_from(val);
        assert_eq!(name.unwrap(), "");
        assert_eq!(filename.unwrap(), "filename.txt");

        let val = br#"form-data; name=";"; filename=";""#;
        let name = ContentDispositionAttr::Name.extract_from(val);
        let filename = ContentDispositionAttr::FileName.extract_from(val);
        assert_eq!(name.unwrap(), ";");
        assert_eq!(filename.unwrap(), ";");
    }

    #[test]
    fn test_content_disposition_name_escaped_quote() {
        let val = br#"form-data; name="my\"field\"name""#;
        let name = ContentDispositionAttr::Name.extract_from(val);
        assert_eq!(name.unwrap(), r#"my"field"name"#);

        let val = br#"form-data; name="myfield\"name""#;
        let name = ContentDispositionAttr::Name.extract_from(val);
        assert_eq!(name.unwrap(), r#"myfield"name"#);
    }
}
