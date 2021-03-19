use std::fmt::{self, Debug, Display, Formatter};

use derive_more::Display;

type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// A set of errors that can occur during parsing multipart stream and in other
/// operations.
#[derive(Display)]
#[non_exhaustive]
pub enum Error {
    /// An unknown field is detected when multipart
    /// [`constraints`](crate::Constraints::allowed_fields) are added.
    #[display(fmt = "unknown field received: {}", "field_name.as_deref().unwrap_or(\"<unknown>\")")]
    UnknownField { field_name: Option<String> },

    /// The field data is found incomplete.
    #[display(
        fmt = "field '{}' received with incomplete data",
        "field_name.as_deref().unwrap_or(\"<unknown>\")"
    )]
    IncompleteFieldData { field_name: Option<String> },

    /// Couldn't read the field headers completely.
    #[display(fmt = "failed to read field complete headers")]
    IncompleteHeaders,

    /// Failed to read headers.
    #[display(fmt = "failed to read headers: {}", _0)]
    ReadHeaderFailed(httparse::Error),

    /// Failed to decode the field's raw header name to
    /// [`HeaderName`](http::header::HeaderName) type.
    #[display(fmt = "failed to decode field's raw header name: {:?} {}", name, cause)]
    DecodeHeaderName { name: String, cause: BoxError },

    /// Failed to decode the field's raw header value to
    /// [`HeaderValue`](http::header::HeaderValue) type.
    #[display(fmt = "failed to decode field's raw header value: {}", cause)]
    DecodeHeaderValue { value: Vec<u8>, cause: BoxError },

    /// Multipart stream is incomplete.
    #[display(fmt = "incomplete multipart stream")]
    IncompleteStream,

    /// The incoming field size exceeded the maximum limit.
    #[display(
        fmt = "field '{}' exceeded the maximum size limit: {} bytes",
        "field_name.as_deref().unwrap_or(\"<unknown>\")",
        limit
    )]
    FieldSizeExceeded { limit: u64, field_name: Option<String> },

    /// The incoming stream size exceeded the maximum limit.
    #[display(fmt = "stream size exceeded the maximum limit: {} bytes", limit)]
    StreamSizeExceeded { limit: u64 },

    /// Stream read failed.
    #[display(fmt = "stream read failed: {}", _0)]
    StreamReadFailed(BoxError),

    /// Failed to lock the multipart shared state for any changes.
    #[display(fmt = "failed to lock multipart state: {}", _0)]
    LockFailure(BoxError),

    /// The `Content-Type` header is not `multipart/form-data`.
    #[display(fmt = "Content-Type is not multipart/form-data")]
    NoMultipart,

    /// Failed to convert the `Content-Type` to [`mime::Mime`] type.
    #[display(fmt = "Failed to convert Content-Type to `mime::Mime` type: {}", _0)]
    DecodeContentType(mime::FromStrError),

    /// No boundary found in `Content-Type` header.
    #[display(fmt = "multipart boundary not found in Content-Type")]
    NoBoundary,

    /// Failed to decode the field data as `JSON` in
    /// [`field.json()`](crate::Field::json) method.
    #[cfg(feature = "json")]
    #[cfg_attr(nightly, doc(cfg(feature = "json")))]
    #[display(fmt = "failed to decode field data as JSON: {}", _0)]
    DecodeJson(serde_json::Error),
}

impl Debug for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        Display::fmt(self, f)
    }
}

impl std::error::Error for Error {}

impl PartialEq for Error {
    fn eq(&self, other: &Self) -> bool {
        self.to_string().eq(&other.to_string())
    }
}

impl Eq for Error {}
