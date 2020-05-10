//! An async parser for `multipart/form-data` content-type in Rust.
//!
//! It accepts a [`Stream`](https://docs.rs/futures/0.3.5/futures/stream/trait.Stream.html) of [`Bytes`](https://docs.rs/bytes/0.5.4/bytes/struct.Bytes.html) as
//! a source, so that It can be plugged into any async Rust environment e.g. any async server.
//!
//! # Examples
//!
//! ```no_run
//! use bytes::Bytes;
//! use futures::stream::Stream;
//! // Import multer types.
//! use multer::Multipart;
//! use std::convert::Infallible;
//! use futures::stream::once;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Generate a byte stream and the boundary from somewhere e.g. server request body.
//!     let (stream, boundary) = get_byte_stream_from_somewhere().await;
//!
//!     // Create a `Multipart` instance from that byte stream and the boundary.
//!     let mut multipart = Multipart::new(stream, boundary);
//!
//!     // Iterate over the fields, use `next_field()` to get the next field.
//!     while let Some(field) = multipart.next_field().await? {
//!         // Get field name.
//!         let name = field.name();
//!         // Get the field's filename if provided in "Content-Disposition" header.
//!         let file_name = field.file_name();
//!
//!         println!("Name: {:?}, File Name: {:?}", name, file_name);
//!
//!         // Process the field data chunks e.g. store them in a file.
//!         while let Some(field_chunk) = field.chunk().await? {
//!             // Do something with field chunk.
//!             println!("Chunk: {:?}", chunk);
//!         }
//!     }
//!
//!     Ok(())
//! }
//!
//! // Generate a byte stream and the boundary from somewhere e.g. server request body.
//! async fn get_byte_stream_from_somewhere() -> (impl Stream<Item = Result<Bytes, Infallible>>, &'static str) {
//!     let data = "--X-BOUNDARY\r\nContent-Disposition: form-data; name=\"My Field\"\r\n\r\nabcd\r\n--X-BOUNDARY--\r\n";
//!     let stream = once(async move { Result::<Bytes, Infallible>::Ok(Bytes::from(data)) });
//!     
//!     (stream, "X-BOUNDARY")
//! }
//! ```
//!
//! ## Usage with [hyper.rs](https://hyper.rs/) server
//!
//! An [example](https://github.com/rousan/multer-rs/blob/master/examples/hyper_server_example.rs) showing usage with [hyper.rs](https://hyper.rs/).
//!
//! For more examples, please visit [examples](https://github.com/rousan/multer-rs/tree/master/examples).

pub use error::Error;
#[doc(hidden)]
pub use error::{ErrorExt, ResultExt};
pub use field::Field;
pub use multipart::Multipart;

mod buffer;
mod constants;
mod error;
mod field;
mod helpers;
mod multipart;
mod state;

/// A Result type often returned from methods that can have `multer` errors.
pub type Result<T> = std::result::Result<T, Error>;

/// Parses the `Content-Type` header to extract the boundary value.
///
/// # Examples
///
/// ```
/// # fn run(){
/// let content_type = "multipart/form-data; boundary=ABCDEFG";
///
/// assert_eq!(multer::parse_boundary(content_type), Ok("ABCDEFG".to_owned()));
/// # }
/// # run();
/// ```
pub fn parse_boundary<T: AsRef<str>>(content_type: T) -> crate::Result<String> {
    let m = content_type
        .as_ref()
        .parse::<mime::Mime>()
        .context("Failed to parse the content type as mime type")?;

    if !(m.type_() == mime::MULTIPART && m.subtype() == mime::FORM_DATA) {
        return Err(crate::Error::new("Content-type is not multipart/form-data"));
    }

    m.get_param(mime::BOUNDARY)
        .map(|name| name.as_str().to_owned())
        .ok_or_else(|| crate::Error::new("No boundary value found"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_boundary() {
        let content_type = "multipart/form-data; boundary=ABCDEFG";
        assert_eq!(parse_boundary(content_type), Ok("ABCDEFG".to_owned()));

        let content_type = "multipart/form-data; boundary=------ABCDEFG";
        assert_eq!(parse_boundary(content_type), Ok("------ABCDEFG".to_owned()));

        let content_type = "boundary=------ABCDEFG";
        assert!(parse_boundary(content_type).is_err());

        let content_type = "text/plain";
        assert!(parse_boundary(content_type).is_err());

        let content_type = "text/plain; boundary=------ABCDEFG";
        assert!(parse_boundary(content_type).is_err());
    }
}
