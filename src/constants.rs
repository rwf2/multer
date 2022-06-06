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

impl ContentDispositionAttr {
    /// Extract ContentDisposition Attribute from header.
    ///
    /// Some older clients may not quote the name or filename, so we allow them, but require them
    /// to be percent encoded. Only allocates if percent decoding, and there are characters that
    /// need to be decoded.
    pub fn extract_from<'h>(&self, header: &'h [u8]) -> Option<Cow<'h, str>> {
        let prefix = match self {
            ContentDispositionAttr::Name => &b"name="[..],
            ContentDispositionAttr::FileName => &b"filename="[..],
        };

        if let Some(i) = memchr::memmem::find(header, prefix) {
            // Check if this is malformed, with `filename` coming first.
            if *self == ContentDispositionAttr::Name && i > 0 && header[i - 1] == b'e' {
                return None;
            }

            let rest = &header[(i + prefix.len())..];
            let j = memchr::memmem::find(rest, b";").unwrap_or(rest.len());
            let content = &rest[..j];
            if content.starts_with(b"\"") && content.ends_with(b"\"") {
                let content = &content[1..content.len() - 1];
                if memchr::memmem::find(content, b"\"").is_some() {
                    return None;
                }
                return std::str::from_utf8(content).map(|s| s.into()).ok();
            } else {
                return percent_encoding::percent_decode(content).decode_utf8().ok();
            }
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
        assert_eq!(name.unwrap(), b"my_field");
        assert!(filename.is_none());
    }

    #[test]
    fn test_content_disposition_extraction() {
        let val = br#"form-data; name="my_field"; filename="file abc.txt""#;
        let name = ContentDispositionAttr::Name.extract_from(val);
        let filename = ContentDispositionAttr::FileName.extract_from(val);
        assert_eq!(name.unwrap(), b"my_field");
        assert_eq!(filename.unwrap(), b"file abc.txt");

        let val = "form-data; name=\"你好\"; filename=\"file abc.txt\"".as_bytes();
        let name = ContentDispositionAttr::Name.extract_from(val);
        let filename = ContentDispositionAttr::FileName.extract_from(val);
        assert_eq!(name.unwrap(), "你好".as_bytes());
        assert_eq!(filename.unwrap(), b"file abc.txt");

        let val = "form-data; name=\"কখগ\"; filename=\"你好.txt\"".as_bytes();
        let name = ContentDispositionAttr::Name.extract_from(val);
        let filename = ContentDispositionAttr::FileName.extract_from(val);
        assert_eq!(name.unwrap(), "কখগ".as_bytes());
        assert_eq!(filename.unwrap(), "你好.txt".as_bytes());
    }

    #[test]
    fn test_content_disposition_file_name_only() {
        // These are technically malformed, as RFC 7578 says the `name`
        // parameter _must_ be included. But okay.
        let val = br#"form-data; filename="file-name.txt""#;
        let name = ContentDispositionAttr::Name.extract_from(val);
        let filename = ContentDispositionAttr::FileName.extract_from(val);
        assert_eq!(filename.unwrap(), b"file-name.txt");
        assert!(name.is_none());

        let val = "form-data; filename=\"কখগ-你好.txt\"".as_bytes();
        let name = ContentDispositionAttr::Name.extract_from(val);
        let filename = ContentDispositionAttr::FileName.extract_from(val);
        assert_eq!(filename.unwrap(), "কখগ-你好.txt".as_bytes());
        assert!(name.is_none());
    }
}
