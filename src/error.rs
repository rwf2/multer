use derive_more::Display;
use std::fmt::{self, Debug, Display, Formatter};

type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// A set of errors that can occur during parsing multipart stream and in other operations.
#[derive(Display)]
#[display(fmt = "multer: {}")]
pub enum Error {
    /// An unknown field is detected when multipart [`constraints`](./struct.Constraints.html#method.allowed_fields) are added.
    #[display(
        fmt = "An unknown field is detected: {}",
        "field_name.as_deref().unwrap_or(\"<unknown>\")"
    )]
    UnknownField { field_name: Option<String> },

    /// The field data is found incomplete.
    #[display(
        fmt = "Incomplete field data for field: {}",
        "field_name.as_deref().unwrap_or(\"<unknown>\")"
    )]
    IncompleteFieldData { field_name: Option<String> },

    /// Couldn't read the field headers completely.
    #[display(fmt = "Incomplete headers, couldn't read the field headers completely")]
    IncompleteHeaders,

    /// Failed to read headers.
    #[display(fmt = "Failed to read headers: {}", _0)]
    ReadHeaderFailed(BoxError),

    /// Failed to decode the field's raw header name to [`HeaderName`](https://docs.rs/http/0.2.1/http/header/struct.HeaderName.html) type.
    #[display(fmt = "Failed to decode the field's raw header name: {}", cause)]
    DecodeHeaderName { name: String, cause: BoxError },

    /// Failed to decode the field's raw header value to [`HeaderValue`](https://docs.rs/http/0.2.1/http/header/struct.HeaderValue.html) type.
    #[display(fmt = "Failed to decode the field's raw header value: {}", cause)]
    DecodeHeaderValue { value: Vec<u8>, cause: BoxError },

    /// Multipart stream is incomplete.
    #[display(fmt = "Multipart stream is incomplete")]
    IncompleteStream,

    /// The incoming field size exceeded the maximum limit.
    #[display(
        fmt = "Incoming field size exceeded the maximum limit: {} bytes, field name: {}",
        limit,
        "field_name.as_deref().unwrap_or(\"<unknown>\")"
    )]
    FieldSizeExceeded { limit: usize, field_name: Option<String> },

    /// The incoming stream size exceeded the maximum limit.
    #[display(fmt = "Stream size exceeded the maximum limit: {} bytes", limit)]
    StreamSizeExceeded { limit: usize },

    /// Stream read failed.
    #[display(fmt = "Stream read failed: {}", _0)]
    StreamReadFailed(BoxError),

    /// Failed to lock the multipart shared state for any changes.
    #[display(fmt = "Couldn't lock the multipart state: {}", _0)]
    LockFailure(BoxError),

    /// The `Content-Type` header is not `multipart/form-data`.
    #[display(fmt = "The Content-Type is not multipart/form-data")]
    NoMultipart,

    /// Failed to convert the `Content-Type` to [`mime::Mime`](https://docs.rs/mime/0.3.16/mime/struct.Mime.html) type.
    #[display(fmt = "Failed to convert the Content-Type to `mime::Mime` type: {}", _0)]
    DecodeContentType(BoxError),

    /// No boundary found in `Content-Type` header.
    #[display(fmt = "No boundary found in Content-Type header")]
    NoBoundary,

    /// Failed to decode the field data as `JSON` in [`field.json()`](./struct.Field.html#method.json) method.
    #[cfg(feature = "json")]
    #[display(fmt = "Failed to decode the field data as JSON: {}", _0)]
    DecodeJson(BoxError),

    #[doc(hidden)]
    __Nonexhaustive,
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
