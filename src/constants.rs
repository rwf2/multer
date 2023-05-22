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
    /// Some older clients may not quote the name or filename, so we allow them,
    /// but require them to be percent encoded. Only allocates if percent
    /// decoding, and there are characters that need to be decoded.
    pub fn extract_from<'h>(&self, header: &'h [u8]) -> Option<&'h [u8]> {
        let prefix = match self {
            ContentDispositionAttr::Name => &b"name="[..],
            ContentDispositionAttr::FileName => &b"filename="[..],
        };
        let prefix_finder = memchr::memmem::Finder::new(prefix);

        let mut rest = header;

        // find form-data;
        rest = Self::skip_pattern(b"form-data", rest);

        // skip separator
        rest = Self::skip_separator(rest);

        let mut last_length = rest.len();
        while rest.len() > prefix.len() {
            let m = prefix_finder.find(rest);
            match m {
                Some(0) => {
                    // skip prefix
                    rest = &rest[prefix.len()..];

                    // parse value
                    let (parsed, _to) = Self::parse_value(rest)?;
                    return Some(parsed);
                }
                _ => {
                    // skip prefix
                    let skip_chars = memchr::memchr(b'=', rest)? + 1;
                    rest = &rest[skip_chars..];

                    // skip value
                    let (_parsed, new_rest) = Self::parse_value(rest)?;
                    rest = new_rest;
                }
            }

            // skip spacer
            rest = Self::skip_separator(rest);

            // break if fix point reached
            if rest.len() >= last_length {
                break;
            }

            last_length = rest.len();
        }

        None
    }

    fn skip_pattern<'a, 'b>(pattern: &'a [u8], mut rest: &'b [u8]) -> &'b [u8] {
        if rest.starts_with(pattern) {
            rest = &rest[pattern.len()..];
        }

        rest
    }

    fn skip_separator(mut rest: &[u8]) -> &[u8] {
        // skip semicolumn
        rest = Self::skip_pattern(b";", rest);

        // skip spaces
        let spaces = rest.iter().position(|v| *v != b' ').unwrap_or(0);
        &rest[spaces..]
    }

    /// Parse initial part of the slice for a header value (both quoted and not).
    /// Returns the parsed value and the remaining content of the slice
    fn parse_value(rest: &[u8]) -> Option<(&[u8], &[u8])> {
        if rest.starts_with(b"\"") {
            let last_quote = memchr::memchr_iter(b'"', rest).skip(1).find(|i| rest[i - 1] != b'\\')?;
            Some((&rest[1..last_quote], &rest[last_quote + 1..]))
        } else {
            let j = memchr::memchr(b';', rest).unwrap_or(rest.len());
            Some((&rest[0..j], &rest[j..]))
        }
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

    #[test]
    fn test_content_distribution_misordered_fields() {
        let val = br#"form-data; filename=file-name.txt; name=file"#;
        let name = ContentDispositionAttr::Name.extract_from(val);
        let filename = ContentDispositionAttr::FileName.extract_from(val);
        assert_eq!(filename.unwrap(), b"file-name.txt");
        assert_eq!(name.unwrap(), b"file");

        let val = br#"form-data; filename="file-name.txt"; name="file""#;
        let name = ContentDispositionAttr::Name.extract_from(val);
        let filename = ContentDispositionAttr::FileName.extract_from(val);
        assert_eq!(filename.unwrap(), b"file-name.txt");
        assert_eq!(name.unwrap(), b"file");

        let val = "form-data; filename=\"你好.txt\"; name=\"কখগ\"".as_bytes();
        let name = ContentDispositionAttr::Name.extract_from(val);
        let filename = ContentDispositionAttr::FileName.extract_from(val);
        assert_eq!(name.unwrap(), "কখগ".as_bytes());
        assert_eq!(filename.unwrap(), "你好.txt".as_bytes());
    }

    #[test]
    fn test_content_disposition_name_unquoted() {
        let val = br#"form-data; name=my_field"#;
        let name = ContentDispositionAttr::Name.extract_from(val);
        let filename = ContentDispositionAttr::FileName.extract_from(val);
        assert_eq!(name.unwrap(), b"my_field");
        assert!(filename.is_none());

        let val = br#"form-data; name=my_field; filename=file-name.txt"#;
        let name = ContentDispositionAttr::Name.extract_from(val);
        let filename = ContentDispositionAttr::FileName.extract_from(val);
        assert_eq!(name.unwrap(), b"my_field");
        assert_eq!(filename.unwrap(), b"file-name.txt");
    }

    #[test]
    fn test_content_disposition_name_quoted() {
        let val = br#"form-data; name="my;f;ield""#;
        let name = ContentDispositionAttr::Name.extract_from(val);
        let filename = ContentDispositionAttr::FileName.extract_from(val);
        assert_eq!(name.unwrap(), b"my;f;ield");
        assert!(filename.is_none());

        let val = br#"form-data; name=my_field; filename="file;name.txt""#;
        let name = ContentDispositionAttr::Name.extract_from(val);
        let filename = ContentDispositionAttr::FileName.extract_from(val);
        assert_eq!(name.unwrap(), b"my_field");
        assert_eq!(filename.unwrap(), b"file;name.txt");

        let val = br#"form-data; name=; filename=filename.txt"#;
        let name = ContentDispositionAttr::Name.extract_from(val);
        let filename = ContentDispositionAttr::FileName.extract_from(val);
        assert_eq!(name.unwrap(), b"");
        assert_eq!(filename.unwrap(), b"filename.txt");

        let val = br#"form-data; name=";"; filename=";""#;
        let name = ContentDispositionAttr::Name.extract_from(val);
        let filename = ContentDispositionAttr::FileName.extract_from(val);
        assert_eq!(name.unwrap(), b";");
        assert_eq!(filename.unwrap(), b";");
    }

    // FIXME: This test should pass.
    #[test]
    #[should_panic]
    fn test_content_disposition_name_escaped_quote() {
        let val = br#"form-data; name="my\"field\"name""#;
        let name = ContentDispositionAttr::Name.extract_from(val);
        assert_eq!(name.unwrap(), b"my\"field\"name");
    }
}
