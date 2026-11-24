use std::collections::HashMap;
use std::convert::TryFrom;

use http::header::{self, HeaderMap, HeaderName, HeaderValue};
use httparse::Header;

use crate::size_limit::checked_size;

pub(crate) fn convert_headers(
    raw_headers: &[Header<'_>],
    limits: &HashMap<HeaderName, u64>,
) -> crate::Result<HeaderMap> {
    let mut headers = HeaderMap::with_capacity(raw_headers.len());

    for raw_header in raw_headers {
        let name = HeaderName::try_from(raw_header.name).map_err(|err| crate::Error::DecodeHeaderName {
            name: raw_header.name.to_owned(),
            cause: err.into(),
        })?;

        if let Some(&limit) = limits.get(&name) {
            let len = raw_header.value.len();
            if checked_size(0, len, limit).is_none() {
                return Err(crate::Error::HeaderSizeExceeded {
                    limit,
                    header: Some((name, len)),
                });
            }
        }

        let value = HeaderValue::try_from(raw_header.value).map_err(|err| crate::Error::DecodeHeaderValue {
            value: raw_header.value.to_owned(),
            cause: err.into(),
        })?;

        headers.insert(name, value);
    }

    Ok(headers)
}

pub(crate) fn parse_content_type(headers: &HeaderMap) -> Option<mime::Mime> {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|val| val.to_str().ok())
        .and_then(|val| val.parse::<mime::Mime>().ok())
}
