use std::collections::HashMap;

use crate::{constants, Result, Error};

/// Represents size limit of the stream to prevent DoS attacks.
///
/// Please refer [`Constraints`](crate::Constraints) for more info.
#[derive(Debug)]
pub struct SizeLimit {
    pub(crate) whole_stream: u64,
    pub(crate) per_field: u64,
    pub(crate) field_map: HashMap<String, u64>,
    pub(crate) per_header: u64,
    pub(crate) header_map: HashMap<http::HeaderName, u64>,
}

impl SizeLimit {
    /// Creates a default size limit which is [`u64::MAX`] for the whole stream
    /// and for each field.
    pub fn new() -> SizeLimit {
        SizeLimit::default()
    }

    /// Sets size limit for the whole stream.
    pub fn whole_stream(mut self, limit: u64) -> SizeLimit {
        self.whole_stream = limit;
        self
    }

    /// Sets size limit for each field.
    pub fn per_field(mut self, limit: u64) -> SizeLimit {
        self.per_field = limit;
        self
    }

    /// Sets size limit for a specific field, it overrides the
    /// [`per_field`](Self::per_field) value for this field.
    ///
    /// It is useful when you want to set a size limit on a textual field which
    /// will be stored in memory to avoid potential DoS attacks from
    /// attackers running the server out of memory.
    pub fn for_field<N: Into<String>>(mut self, field_name: N, limit: u64) -> SizeLimit {
        self.field_map.insert(field_name.into(), limit);
        self
    }

    /// Sets size limit for each header, irrespective of its name. This limit is
    /// applied to the entire header, including the header name, value, and any
    /// control characters.
    pub fn per_header(mut self, limit: u64) -> SizeLimit {
        self.per_header = limit;
        self
    }

    /// Sets a size limit for the value of a header given its name. This limit
    /// is applied in addition to the [`per_header`](Self::per_header) limit
    /// when the header name matches.
    ///
    /// An error is returned if the header name is invalid.
    ///
    /// # Examples
    ///
    /// ```
    /// use multer::SizeLimit;
    ///
    /// // Set a size limit of 512 bytes for the Content-Type header.
    /// let size_limit = SizeLimit::new();
    /// size_limit.try_for_header("Content-Type", 512).unwrap();
    /// ```
    pub fn try_for_header<N: AsRef<str>>(mut self, name: N, limit: u64) -> Result<SizeLimit> {
        let name = name.as_ref();
        let header_name = http::HeaderName::from_bytes(name.as_ref())
            .map_err(|e| Error::DecodeHeaderName {
                name: name.to_owned(),
                cause: e.into(),
            })?;

        self.header_map.insert(header_name, limit);
        Ok(self)
    }

    /// Exactly the same as [`try_for_header()`](Self::try_for_header) except
    /// infallible.
    ///
    /// # Examples
    ///
    /// ```
    /// use multer::SizeLimit;
    ///
    /// // Set a size limit of 512 bytes for the Content-Type header.
    /// let size_limit = SizeLimit::new();
    /// size_limit.for_header(http::header::CONTENT_TYPE, 512);
    /// ```
    pub fn for_header(mut self, name: http::HeaderName, limit: u64) -> SizeLimit {
        self.header_map.insert(name, limit);
        self
    }

    pub(crate) fn extract_size_limit_for(&self, field: Option<&str>) -> u64 {
        field
            .and_then(|field| self.field_map.get(&field.to_owned()))
            .copied()
            .unwrap_or(self.per_field)
    }
}

impl Default for SizeLimit {
    fn default() -> Self {
        SizeLimit {
            whole_stream: constants::DEFAULT_WHOLE_STREAM_SIZE_LIMIT,
            per_field: constants::DEFAULT_PER_FIELD_SIZE_LIMIT,
            field_map: HashMap::default(),
            per_header: constants::DEFAULT_PER_HEADER_SIZE_LIMIT,
            header_map: HashMap::default(),
        }
    }
}
