use crate::error::ResultExt;
use http::header::{HeaderMap, HeaderName, HeaderValue};
use httparse::Header;
use std::convert::TryFrom;

pub(crate) fn convert_raw_headers_to_header_map(raw_headers: &[Header]) -> crate::Result<HeaderMap> {
    let mut headers = HeaderMap::with_capacity(raw_headers.len());

    for raw_header in raw_headers {
        let name = HeaderName::try_from(raw_header.name)
            .context("Couldn't convert the raw header name to `HeaderName` type")?;

        let value = HeaderValue::try_from(raw_header.value)
            .context("Couldn't convert the raw header value to `HeaderValue` type")?;

        headers.insert(name, value);
    }

    Ok(headers)
}
