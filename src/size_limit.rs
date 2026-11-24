use std::collections::HashMap;
use std::convert::TryFrom;

use crate::{constants, Error, Result};

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

    /// Limits all bytes fetched from the source, including preamble and skipped fields.
    /// Each fetched stream item counts in full, including any bytes after the closing
    /// boundary. Parsing stops at the closing boundary without fetching later items.
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

    /// Limits the raw header section of each part, including all names, values,
    /// separators and the terminating blank line. A section exactly at the limit
    /// is accepted. The default is [`u64::MAX`].
    pub fn per_header(mut self, limit: u64) -> SizeLimit {
        self.per_header = limit;
        self
    }

    /// Like [`for_header()`](Self::for_header), limits each parsed value of the
    /// named header to `limit` bytes. Returns an error if the name is invalid.
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
        let header_name = http::HeaderName::from_bytes(name.as_ref()).map_err(|e| Error::DecodeHeaderName {
            name: name.to_owned(),
            cause: e.into(),
        })?;

        self.header_map.insert(header_name, limit);
        Ok(self)
    }

    /// Limits each parsed value of `name` to `limit` bytes. If the header
    /// occurs more than once in a part, every value must fit. The field's
    /// header map retains the last value for that name.
    ///
    /// Use [`per_header()`](Self::per_header) to bound the raw header section
    /// as Multer reads it.
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
            .and_then(|field| self.field_map.get(field))
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

// Convert before comparing so even a limit larger than usize::MAX is preserved.
pub(crate) fn checked_size(total: u64, additional: usize, limit: u64) -> Option<u64> {
    total
        .checked_add(u64::try_from(additional).ok()?)
        .filter(|&size| size <= limit)
}
