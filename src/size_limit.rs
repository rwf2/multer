use crate::constants;
use std::collections::HashMap;

/// Represents size limit of the stream to prevent DDoS attack.
///
/// Please refer [`Constraints`](./struct.Constraints.html) for more info.
pub struct SizeLimit {
    pub(crate) whole_stream: usize,
    pub(crate) per_field: usize,
    pub(crate) field_map: HashMap<String, usize>,
}

impl SizeLimit {
    /// Creates a default size limit which is [`usize::MAX`](https://doc.rust-lang.org/stable/std/primitive.usize.html#associatedconstant.MAX) for the whole stream
    /// and for each field.
    pub fn new() -> SizeLimit {
        SizeLimit::default()
    }

    /// Sets size limit for the whole stream.
    pub fn whole_stream(mut self, limit: usize) -> SizeLimit {
        self.whole_stream = limit;
        self
    }

    /// Sets size limit for each field.
    pub fn per_field(mut self, limit: usize) -> SizeLimit {
        self.per_field = limit;
        self
    }

    /// Sets size limit for a specific field, it overrides the `per_field` value for this field.
    ///
    /// It is useful when you want to set a size limit on a textual field which will be stored in memory
    /// to avoid potential `DDoS attack` from attackers running the server out of memory.
    pub fn for_field<N: Into<String>>(mut self, field_name: N, limit: usize) -> SizeLimit {
        self.field_map.insert(field_name.into(), limit);
        self
    }

    pub(crate) fn extract_size_limit_for(&self, field: Option<&str>) -> usize {
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
        }
    }
}
