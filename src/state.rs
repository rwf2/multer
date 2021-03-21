use crate::buffer::StreamBuffer;

#[derive(Debug)]
pub(crate) struct MultipartState {
    pub(crate) buffer: StreamBuffer,
    pub(crate) boundary: String,
    pub(crate) stage: StreamingStage,
    pub(crate) next_field_idx: usize,
    pub(crate) curr_field_name: Option<String>,
    pub(crate) curr_field_size_limit: u64,
    pub(crate) curr_field_size_counter: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StreamingStage {
    FindingFirstBoundary,
    ReadingBoundary,
    DeterminingBoundaryType,
    ReadingTransportPadding,
    ReadingFieldHeaders,
    ReadingFieldData,
    Eof,
}
