use std::fmt;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{ready, Context, Poll};

use bytes::{Buf, Bytes, BytesMut};
use futures_util::stream::Stream;
#[cfg(feature = "tokio-io")]
use {bytes::BufMut, tokio::io::AsyncRead, tokio_util::io::poll_read_buf};

use crate::error::BoxError;
use crate::size_limit::checked_size;

const REFILL_BYTES: usize = 16 * 1024;
pub(crate) const REFILLS_PER_POLL: usize = 32;

pub(crate) enum Input<'r> {
    Stream(Pin<Box<dyn Stream<Item = Result<Bytes, BoxError>> + Send + 'r>>),
    #[cfg(feature = "tokio-io")]
    Reader(Box<dyn AsyncRead + Unpin + Send + 'r>),
}

// These failures can be observed by both the live field and its parent.
pub(crate) enum ReadError {
    Source(Arc<BoxError>),
    StreamSize,
    FieldSize,
    IncompleteField,
}

pub(crate) struct StreamBuffer<'r> {
    pub(crate) eof: bool,
    pub(crate) buf: BytesMut,
    input: Input<'r>,
    remainder: Bytes,
    stream_size_limit: u64,
    stream_size: u64,
}

impl<'r> StreamBuffer<'r> {
    pub fn new(input: Input<'r>, stream_size_limit: u64) -> Self {
        Self {
            eof: false,
            buf: BytesMut::new(),
            input,
            remainder: Bytes::new(),
            stream_size_limit,
            stream_size: 0,
        }
    }

    pub fn poll_refill(&mut self, cx: &mut Context<'_>, budget: &mut usize) -> Poll<Result<(), ReadError>> {
        if *budget == 0 {
            cx.waker().wake_by_ref();
            return Poll::Pending;
        }
        *budget -= 1;

        match &mut self.input {
            Input::Stream(stream) => {
                if self.remainder.is_empty() {
                    let item = match ready!(stream.as_mut().poll_next(cx)) {
                        Some(Ok(item)) => item,
                        Some(Err(err)) => return Poll::Ready(Err(ReadError::Source(Arc::new(err)))),
                        None => {
                            self.eof = true;
                            return Poll::Ready(Ok(()));
                        }
                    };
                    self.stream_size = checked_size(self.stream_size, item.len(), self.stream_size_limit)
                        .ok_or(ReadError::StreamSize)?;
                    self.remainder = item;
                }

                let len = self.remainder.len().min(REFILL_BYTES);
                self.buf.extend_from_slice(&self.remainder[..len]);
                if len == self.remainder.len() {
                    self.remainder = Bytes::new();
                } else {
                    self.remainder.advance(len);
                }
            }
            #[cfg(feature = "tokio-io")]
            Input::Reader(reader) => {
                if self.buf.capacity() == self.buf.len() {
                    self.buf.reserve(4096);
                }
                let mut dest = (&mut self.buf).limit(REFILL_BYTES);
                let len = ready!(poll_read_buf(Pin::new(reader), cx, &mut dest))
                    .map_err(|err| ReadError::Source(Arc::new(err.into())))?;
                self.eof = len == 0;
                self.stream_size =
                    checked_size(self.stream_size, len, self.stream_size_limit).ok_or(ReadError::StreamSize)?;
            }
        }
        Poll::Ready(Ok(()))
    }

    pub fn discard(&mut self) {
        self.remainder = Bytes::new();
        self.buf = BytesMut::new();
    }

    pub fn advance_past_transport_padding(&mut self) {
        let padding = self.buf.iter().take_while(|&&b| b == b' ' || b == b'\t').count();
        self.buf.advance(padding);
    }

    pub fn read_field_data(&mut self, marker: &[u8]) -> Option<(bool, Bytes)> {
        if self.buf.is_empty() {
            return None;
        }
        if let Some(idx) = memchr::memmem::find(&self.buf, marker) {
            trace!("field boundary at byte {}", idx);
            let bytes = self.buf.split_to(idx).freeze();
            self.buf.advance(marker.len());
            return Some((true, bytes));
        }

        // Only a trailing prefix can become a delimiter after the next refill.
        let start = self.buf.len().saturating_sub(marker.len() - 1);
        let tail = &self.buf[start..];
        let keep_from = memchr::memchr_iter(b'\r', tail)
            .find(|&pos| marker.starts_with(&tail[pos..]))
            .map_or(self.buf.len(), |pos| start + pos);
        (keep_from > 0).then(|| (false, self.buf.split_to(keep_from).freeze()))
    }
}

impl fmt::Debug for StreamBuffer<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StreamBuffer").finish()
    }
}
