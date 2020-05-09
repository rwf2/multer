use crate::buffer::StreamBuffer;
use std::task::Waker;

pub(crate) struct MultipartState<S> {
    pub(crate) buffer: StreamBuffer<S>,
    pub(crate) boundary: String,
    pub(crate) stage: StreamingStage,
    pub(crate) is_prev_field_consumed: bool,
    pub(crate) next_field_waker: Option<Waker>,
    pub(crate) next_field_idx: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StreamingStage {
    CleaningPrevFieldData,
    ReadingBoundary,
    ReadingFieldHeaders,
    ReadingFieldData,
    Eof,
}
