use once_cell::sync::Lazy;
use regex::bytes::Regex;

pub(crate) const DEFAULT_WHOLE_STREAM_SIZE_LIMIT: u64 = std::u64::MAX;
pub(crate) const DEFAULT_PER_FIELD_SIZE_LIMIT: u64 = std::u64::MAX;

pub(crate) const MAX_HEADERS: usize = 32;
pub(crate) const BOUNDARY_EXT: &str = "--";
pub(crate) const CR: &str = "\r";
#[allow(dead_code)]
pub(crate) const LF: &str = "\n";
pub(crate) const CRLF: &str = "\r\n";
pub(crate) const CRLF_CRLF: &str = "\r\n\r\n";

pub(crate) static CONTENT_DISPOSITION_FIELD_NAME_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"(?-u)name="([^"]+)""#).unwrap());
pub(crate) static CONTENT_DISPOSITION_FILE_NAME_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"(?-u)filename="([^"]+)""#).unwrap());

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_content_disposition_field_name_re() {
        let val = br#"form-data; name="my_field""#;
        let name = CONTENT_DISPOSITION_FIELD_NAME_RE.captures(val).unwrap();
        assert_eq!(name.get(1).unwrap().as_bytes(), b"my_field");

        let val = br#"form-data; name="my field""#;
        let name = CONTENT_DISPOSITION_FIELD_NAME_RE.captures(val).unwrap();
        assert_eq!(name.get(1).unwrap().as_bytes(), b"my field");

        let val = br#"form-data; name="my_field"; filename="file abc.txt""#;
        let name = CONTENT_DISPOSITION_FIELD_NAME_RE.captures(val).unwrap();
        assert_eq!(name.get(1).unwrap().as_bytes(), b"my_field");

        let val = br#"form-data; name="my field"; filename="file abc.txt""#;
        let name = CONTENT_DISPOSITION_FIELD_NAME_RE.captures(val).unwrap();
        assert_eq!(name.get(1).unwrap().as_bytes(), b"my field");

        let val = "form-data; name=\"你好\"; filename=\"file abc.txt\"".as_bytes();
        let name = CONTENT_DISPOSITION_FIELD_NAME_RE.captures(val).unwrap();
        assert_eq!(name.get(1).unwrap().as_bytes(), "你好".as_bytes());

        let val = "form-data; name=\"কখগ\"; filename=\"你好.txt\"".as_bytes();
        let name = CONTENT_DISPOSITION_FIELD_NAME_RE.captures(val).unwrap();
        assert_eq!(name.get(1).unwrap().as_bytes(), "কখগ".as_bytes());
    }

    #[test]
    fn test_content_disposition_file_name_re() {
        let val = br#"form-data; name="my_field"; filename="file_name.txt""#;
        let file_name = CONTENT_DISPOSITION_FILE_NAME_RE.captures(val).unwrap();
        assert_eq!(file_name.get(1).unwrap().as_bytes(), b"file_name.txt");

        let val = br#"form-data; name="my_field"; filename="file name.txt""#;
        let file_name = CONTENT_DISPOSITION_FILE_NAME_RE.captures(val).unwrap();
        assert_eq!(file_name.get(1).unwrap().as_bytes(), b"file name.txt");

        let val = br#"form-data; filename="file-name.txt""#;
        let file_name = CONTENT_DISPOSITION_FILE_NAME_RE.captures(val).unwrap();
        assert_eq!(file_name.get(1).unwrap().as_bytes(), b"file-name.txt");

        let val = "form-data; filename=\"কখগ-你好.txt\"".as_bytes();
        let file_name = CONTENT_DISPOSITION_FILE_NAME_RE.captures(val).unwrap();
        assert_eq!(file_name.get(1).unwrap().as_bytes(), "কখগ-你好.txt".as_bytes());
    }
}
