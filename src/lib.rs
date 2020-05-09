//! A multipart/form-data parser and writer for tokio.rs in Rust
//!
//! # Examples
//!
//! ```
//! use multer_rs;
//!
//! # fn run() {
//! println!("{}", multer_rs::add(2, 3));
//! # }
//! # run();
//! ```

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
pub fn parse_boundary<T: AsRef<str>>(content_type: T) -> crate::Result<String> {
    let m = content_type
        .as_ref()
        .parse::<mime::Mime>()
        .context("Failed to parse the content type as mime type")?;

    if !(m.type_() == mime::MULTIPART_FORM_DATA.type_() && m.subtype() == mime::MULTIPART_FORM_DATA.subtype()) {
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
