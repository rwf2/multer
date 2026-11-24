use std::fmt;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use bytes::{Buf, Bytes, BytesMut};
use futures_util::ready;
use futures_util::stream::Stream;
use memchr::memmem::Finder;
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
    marker_start: Finder<'static>,
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
            marker_start: Finder::new(b"\r\n--"),
            remainder: Bytes::new(),
            stream_size_limit,
            stream_size: 0,
        }
    }

    // Return an owned payload slice when the stream supplies distinguishable body
    // bytes. Framing and reader input go into the parser buffer.
    pub fn poll_refill(
        &mut self,
        cx: &mut Context<'_>,
        budget: &mut usize,
        marker: Option<&[u8]>,
    ) -> Poll<Result<Option<Bytes>, ReadError>> {
        if *budget == 0 {
            cx.waker().wake_by_ref();
            return Poll::Pending;
        }
        *budget -= 1;

        match &mut self.input {
            Input::Stream(stream) => {
                let mut item = if self.remainder.is_empty() {
                    let item = match ready!(stream.as_mut().poll_next(cx)) {
                        Some(Ok(item)) => item,
                        Some(Err(err)) => return Poll::Ready(Err(ReadError::Source(Arc::new(err)))),
                        None => {
                            self.eof = true;
                            return Poll::Ready(Ok(None));
                        }
                    };
                    self.stream_size = checked_size(self.stream_size, item.len(), self.stream_size_limit)
                        .ok_or(ReadError::StreamSize)?;
                    item
                } else {
                    std::mem::take(&mut self.remainder)
                };

                if item.len() > REFILL_BYTES {
                    self.remainder = item.split_off(REFILL_BYTES);
                }
                if let Some(marker) = marker {
                    if self.buf.is_empty()
                        && !item.is_empty()
                        && payload_span(&item, marker, &self.marker_start).0 == item.len()
                    {
                        return Poll::Ready(Ok(Some(item)));
                    }
                }
                self.buf.extend_from_slice(&item);
                Poll::Ready(Ok(None))
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
                Poll::Ready(Ok(None))
            }
        }
    }

    pub fn discard(&mut self) {
        self.remainder = Bytes::new();
        self.buf = BytesMut::new();
    }

    pub fn advance_past_transport_padding(&mut self) {
        let padding = self.buf.iter().take_while(|&&b| b == b' ' || b == b'\t').count();
        self.buf.advance(padding);
    }

    pub fn poll_field_data(
        &mut self,
        cx: &mut Context<'_>,
        budget: &mut usize,
        marker: &[u8],
    ) -> Poll<Result<(bool, Bytes), ReadError>> {
        loop {
            if !self.buf.is_empty() {
                let (len, done) = payload_span(&self.buf, marker, &self.marker_start);
                if len > 0 || done {
                    let bytes = self.buf.split_to(len).freeze();
                    if done {
                        self.buf.advance(marker.len());
                    }
                    return Poll::Ready(Ok((done, bytes)));
                }
            }
            if self.eof {
                return Poll::Ready(Err(ReadError::IncompleteField));
            }
            if let Some(item) = ready!(self.poll_refill(cx, budget, Some(marker)))? {
                return Poll::Ready(Ok((false, item)));
            }
        }
    }
}

// Returns the distinguishable payload length and whether a full marker follows.
fn payload_span(buf: &[u8], needle: &[u8], marker_start: &Finder<'_>) -> (usize, bool) {
    let first = match marker_start.find(buf) {
        Some(pos) => pos,
        None => {
            // No CRLF-- occurs in this span. At most its final three bytes
            // can start a marker which continues into the next refill.
            let overlap = match buf.last() {
                Some(b'\r') => 1,
                Some(b'\n') if buf.ends_with(b"\r\n") => 2,
                Some(b'-') if buf.ends_with(b"\r\n-") => 3,
                _ => 0,
            };
            return (buf.len() - overlap, false);
        }
    };
    if buf[first..].starts_with(needle) {
        trace!("field boundary at byte {}", first);
        return (first, true);
    }
    if let Some(idx) = memchr::memmem::find(&buf[first + 1..], needle) {
        return (first + 1 + idx, true);
    }
    // Only a trailing prefix can become a delimiter after the next refill.
    let start = buf.len().saturating_sub(needle.len() - 1);
    let tail = &buf[start..];
    let len = memchr::memchr_iter(b'\r', tail)
        .find(|&pos| needle.starts_with(&tail[pos..]))
        .map_or(buf.len(), |pos| start + pos);
    (len, false)
}

impl fmt::Debug for StreamBuffer<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StreamBuffer").finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_spans_match_a_direct_search() {
        let start = Finder::new(b"\r\n--");
        for needle in [&b"\r\n--B"[..], b"\r\n--ABAB", b"\r\n--B\r\n--B"] {
            let text = [
                b"\rX\nY\r\r\n\r\n--A\r\n--ABAX\r\n--".as_slice(),
                needle,
                needle,
                b"\r\n-",
            ]
            .concat();
            for from in 0..=text.len() {
                for to in from..=text.len() {
                    let buf = &text[from..to];
                    let expected = match buf.windows(needle.len()).position(|window| window == needle) {
                        Some(pos) => (pos, true),
                        None => {
                            let overlap = (1..needle.len())
                                .rev()
                                .find(|&n| buf.ends_with(&needle[..n]))
                                .unwrap_or(0);
                            (buf.len() - overlap, false)
                        }
                    };
                    assert_eq!(payload_span(buf, needle, &start), expected);
                }
            }
        }
    }

    #[test]
    fn whole_stream_counter_accepts_max_and_rejects_overflow() {
        let source = futures_util::stream::iter([Ok(Bytes::from_static(b"a")), Ok(Bytes::from_static(b"b"))]);
        let mut buffer = StreamBuffer::new(Input::Stream(Box::pin(source)), u64::MAX);
        buffer.stream_size = u64::MAX - 1;
        let mut cx = Context::from_waker(futures_util::task::noop_waker_ref());
        let mut budget = REFILLS_PER_POLL;
        assert!(matches!(
            buffer.poll_refill(&mut cx, &mut budget, None),
            Poll::Ready(Ok(None))
        ));
        assert_eq!(buffer.stream_size, u64::MAX);
        assert!(matches!(
            buffer.poll_refill(&mut cx, &mut budget, None),
            Poll::Ready(Err(ReadError::StreamSize))
        ));
    }
}
