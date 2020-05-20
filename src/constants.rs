use lazy_static::lazy_static;
use regex::Regex;

pub(crate) const DEFAULT_WHOLE_STREAM_SIZE_LIMIT: u64 = u64::MAX;
pub(crate) const DEFAULT_PER_FIELD_SIZE_LIMIT: u64 = u64::MAX;

pub(crate) const MAX_HEADERS: usize = 32;
pub(crate) const BOUNDARY_EXT: &'static str = "--";
pub(crate) const CR: &'static str = "\r";
#[allow(dead_code)]
pub(crate) const LF: &'static str = "\n";
pub(crate) const CRLF: &'static str = "\r\n";
pub(crate) const CRLF_CRLF: &'static str = "\r\n\r\n";

lazy_static! {
    pub(crate) static ref CONTENT_DISPOSITION_FIELD_NAME_RE: Regex = Regex::new(r#"name="([^"]+)""#).unwrap();
    pub(crate) static ref CONTENT_DISPOSITION_FILE_NAME_RE: Regex = Regex::new(r#"filename="([^"]+)""#).unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_content_disposition_field_name_re() {
        let val = r#"form-data; name="my_field""#;
        let name = CONTENT_DISPOSITION_FIELD_NAME_RE.captures(val).unwrap();
        assert_eq!(name.get(1).unwrap().as_str(), "my_field");

        let val = r#"form-data; name="my field""#;
        let name = CONTENT_DISPOSITION_FIELD_NAME_RE.captures(val).unwrap();
        assert_eq!(name.get(1).unwrap().as_str(), "my field");

        let val = r#"form-data; name="my_field"; filename="file abc.txt""#;
        let name = CONTENT_DISPOSITION_FIELD_NAME_RE.captures(val).unwrap();
        assert_eq!(name.get(1).unwrap().as_str(), "my_field");

        let val = r#"form-data; name="my field"; filename="file abc.txt""#;
        let name = CONTENT_DISPOSITION_FIELD_NAME_RE.captures(val).unwrap();
        assert_eq!(name.get(1).unwrap().as_str(), "my field");
    }

    #[test]
    fn test_content_disposition_file_name_re() {
        let val = r#"form-data; name="my_field"; filename="file_name.txt""#;
        let file_name = CONTENT_DISPOSITION_FILE_NAME_RE.captures(val).unwrap();
        assert_eq!(file_name.get(1).unwrap().as_str(), "file_name.txt");

        let val = r#"form-data; name="my_field"; filename="file name.txt""#;
        let file_name = CONTENT_DISPOSITION_FILE_NAME_RE.captures(val).unwrap();
        assert_eq!(file_name.get(1).unwrap().as_str(), "file name.txt");

        let val = r#"form-data; filename="file-name.txt""#;
        let file_name = CONTENT_DISPOSITION_FILE_NAME_RE.captures(val).unwrap();
        assert_eq!(file_name.get(1).unwrap().as_str(), "file-name.txt");
    }
}
