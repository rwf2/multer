use crate::constants;
use http::header::{self, HeaderMap};

pub(crate) struct ContentDisposition {
    pub(crate) field_name: Option<String>,
    pub(crate) file_name: Option<String>,
}

impl ContentDisposition {
    pub fn parse(headers: &HeaderMap) -> ContentDisposition {
        let content_disposition = headers.get(header::CONTENT_DISPOSITION).map(|val| val.as_bytes());

        let field_name = content_disposition
            .and_then(|val| constants::CONTENT_DISPOSITION_FIELD_NAME_RE.captures(val))
            .and_then(|cap| cap.get(1))
            .map(|m| m.as_bytes().to_vec())
            .and_then(|bytes| String::from_utf8(bytes).ok());

        let file_name = content_disposition
            .and_then(|val| constants::CONTENT_DISPOSITION_FILE_NAME_RE.captures(val))
            .and_then(|cap| cap.get(1))
            .map(|m| m.as_bytes().to_vec())
            .and_then(|bytes| String::from_utf8(bytes).ok());

        ContentDisposition { field_name, file_name }
    }
}
