pub(crate) const DEFAULT_WHOLE_STREAM_SIZE_LIMIT: u64 = u64::MAX;
pub(crate) const DEFAULT_PER_FIELD_SIZE_LIMIT: u64 = u64::MAX;
pub(crate) const DEFAULT_PER_HEADER_SIZE_LIMIT: u64 = u64::MAX;

pub(crate) const MAX_HEADERS: usize = 32;
pub(crate) const BOUNDARY_EXT: &str = "--";
pub(crate) const CRLF: &str = "\r\n";
pub(crate) const CRLF_CRLF: &str = "\r\n\r\n";
