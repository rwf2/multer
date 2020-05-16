use crate::constants;
use http::header::{self, HeaderMap};

pub(crate) struct ContentDisposition {
    pub(crate) field_name: Option<String>,
    pub(crate) file_name: Option<String>,
}

impl ContentDisposition {
    pub fn parse(headers: &HeaderMap) -> ContentDisposition {
        let content_disposition = headers
            .get(header::CONTENT_DISPOSITION)
            .and_then(|val| val.to_str().ok());

        let field_name = content_disposition
            .and_then(|val| constants::CONTENT_DISPOSITION_FIELD_NAME_RE.captures(val))
            .and_then(|cap| cap.get(1))
            .map(|m| m.as_str().to_owned());

        let file_name = content_disposition
            .and_then(|val| constants::CONTENT_DISPOSITION_FILE_NAME_RE.captures(val))
            .and_then(|cap| cap.get(1))
            .map(|m| m.as_str().to_owned());

        ContentDisposition { field_name, file_name }
    }
}
