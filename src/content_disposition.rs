use http::header::{self, HeaderMap};

use crate::constants::ContentDispositionAttr;

#[derive(Debug)]
pub(crate) struct ContentDisposition {
    pub(crate) field_name: Option<String>,
    pub(crate) file_name: Option<String>,
}

impl ContentDisposition {
    pub fn parse(headers: &HeaderMap) -> ContentDisposition {
        let content_disposition = headers.get(header::CONTENT_DISPOSITION).map(|val| val.as_bytes());

        let field_name = content_disposition
            .and_then(|val| ContentDispositionAttr::Name.extract_from(val))
            .map(|attr| attr.into_owned());

        let file_name = content_disposition
            .and_then(|val| ContentDispositionAttr::FileName.extract_from(val))
            .map(|attr| attr.into_owned());

        ContentDisposition { field_name, file_name }
    }
}
