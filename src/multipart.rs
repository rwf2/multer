use std::sync::Arc;
use std::task::{Context, Poll};

use bytes::{Buf, Bytes};
use futures_util::future;
use futures_util::ready;
use futures_util::stream::{Stream, TryStreamExt};
use spin::mutex::spin::SpinMutex as Mutex;
#[cfg(feature = "tokio-io")]
use tokio::io::AsyncRead;

use http::HeaderMap;
use memchr::memmem::Finder;

use crate::buffer::{Input, ReadError, StreamBuffer, REFILLS_PER_POLL};
use crate::constraints::Constraints;
use crate::content_disposition::ContentDisposition;
use crate::error::Error;
use crate::field::Field;
use crate::size_limit::checked_size;
use crate::{constants, helpers, Result};

/// Represents the implementation of `multipart/form-data` formatted data.
///
/// This will parse the source stream into [`Field`] instances via
/// [`next_field()`](Self::next_field).
///
/// Fields and payload chunks are returned as soon as their bytes are available.
/// A later read can report a source or size error after a field or payload prefix
/// has been delivered. Drive the parser to `Ok(None)` to confirm completion.
/// The closing boundary completes the form without waiting for transport EOF.
///
/// A source, framing or limit error ends parsing. When a field observes it first,
/// dropping that field lets the parent report the same cause once. After the parent
/// reports a terminal error, each later call returns [`Error::MultipartFailed`].
/// Dropping a healthy field skips its unread payload under its usual size limit.
/// [`Error::UnknownField`] and a fully read field's JSON conversion error allow
/// iteration to continue.
///
/// # Field Exclusivity
///
/// A `Field` represents a raw, self-decoding stream into multipart data. As
/// such, only _one_ `Field` from a given `Multipart` instance may be live at
/// once. That is, a `Field` emitted by `next_field()` must be dropped before
/// calling `next_field()` again. Failure to do so will result in an error.
///
/// ```rust
/// use std::convert::Infallible;
///
/// use bytes::Bytes;
/// use futures_util::stream::once;
/// use multer::Multipart;
///
/// # async fn run() {
/// let data = "--X-BOUNDARY\r\nContent-Disposition: form-data; \
///     name=\"my_text_field\"\r\n\r\nabcd\r\n--X-BOUNDARY--\r\n";
///
/// let stream = once(async move { Result::<Bytes, Infallible>::Ok(Bytes::from(data)) });
/// let mut multipart = Multipart::new(stream, "X-BOUNDARY");
///
/// let field1 = multipart.next_field().await;
/// let field2 = multipart.next_field().await;
///
/// assert!(field2.is_err());
/// # }
/// # tokio::runtime::Runtime::new().unwrap().block_on(run());
/// ```
///
/// # Examples
///
/// ```
/// use std::convert::Infallible;
///
/// use bytes::Bytes;
/// use futures_util::stream::once;
/// use multer::Multipart;
///
/// # async fn run() {
/// let data = "--X-BOUNDARY\r\nContent-Disposition: form-data; \
///     name=\"my_text_field\"\r\n\r\nabcd\r\n--X-BOUNDARY--\r\n";
///
/// let stream = once(async move { Result::<Bytes, Infallible>::Ok(Bytes::from(data)) });
/// let mut multipart = Multipart::new(stream, "X-BOUNDARY");
///
/// while let Some(field) = multipart.next_field().await.unwrap() {
///     println!("Field: {:?}", field.text().await)
/// }
/// # }
/// # tokio::runtime::Runtime::new().unwrap().block_on(run());
/// ```
#[derive(Debug)]
pub struct Multipart<'r> {
    state: Arc<Mutex<MultipartState<'r>>>,
}

#[derive(Debug)]
pub(crate) struct MultipartState<'r> {
    buffer: StreamBuffer<'r>,
    // The opening and ordinary boundary lines start after the leading CRLF.
    marker: Finder<'static>,
    stage: StreamingStage,
    next_field_idx: usize,
    curr_field_name: Option<String>,
    curr_field_size_limit: u64,
    curr_field_size_counter: u64,
    constraints: Constraints,
}

#[derive(Debug)]
enum StreamingStage {
    FindingFirstBoundary(OpeningSearch),
    DeterminingBoundaryType,
    ReadingTransportPadding,
    ReadingFieldHeaders { scanned: usize },
    ReadingFieldData,
    Eof,
    Failed(Option<Box<Error>>),
}

#[derive(Debug, Default)]
struct OpeningSearch {
    matched: usize,
    // Prefix lengths for KMP fallback. Allocated only on a fragmented opening.
    prefixes: Vec<usize>,
}

impl OpeningSearch {
    fn consume(&mut self, buf: &mut bytes::BytesMut, marker: &[u8]) -> bool {
        if buf.is_empty() {
            return false;
        }
        if self.prefixes.is_empty() {
            // A form can start directly with "--boundary". Every marker in
            // the preamble must instead include the preceding CRLF.
            if buf.starts_with(&marker[2..]) {
                buf.advance(marker.len() - 2);
                return true;
            }
            if let Some(pos) = memchr::memmem::find(buf, marker) {
                buf.advance(pos + marker.len());
                return true;
            }
            self.prefixes.resize(marker.len(), 0);
            let mut matched = 0;
            for i in 1..marker.len() {
                while matched > 0 && marker[matched] != marker[i] {
                    matched = self.prefixes[matched - 1];
                }
                matched += usize::from(marker[matched] == marker[i]);
                self.prefixes[i] = matched;
            }
            // Treat the start of the stream as if it followed a CRLF, even
            // when the first marker arrives in pieces. KMP carries any later
            // line start across refills as part of the marker itself.
            self.matched = 2;
        }

        for (i, &byte) in buf.iter().enumerate() {
            while self.matched > 0 && marker[self.matched] != byte {
                self.matched = self.prefixes[self.matched - 1];
            }
            self.matched += usize::from(marker[self.matched] == byte);
            if self.matched == marker.len() {
                buf.advance(i + 1);
                return true;
            }
        }
        buf.clear();
        false
    }
}

impl<'r> Multipart<'r> {
    /// Construct a new `Multipart` instance with the given [`Bytes`] stream and
    /// the boundary.
    pub fn new<S, O, E, B>(stream: S, boundary: B) -> Self
    where
        S: Stream<Item = Result<O, E>> + Send + 'r,
        O: Into<Bytes> + 'static,
        E: Into<Box<dyn std::error::Error + Send + Sync>> + 'r,
        B: Into<String>,
    {
        Multipart::with_constraints(stream, boundary, Constraints::default())
    }

    /// Construct a new `Multipart` instance with the given [`Bytes`] stream and
    /// the boundary.
    pub fn with_constraints<S, O, E, B>(stream: S, boundary: B, constraints: Constraints) -> Self
    where
        S: Stream<Item = Result<O, E>> + Send + 'r,
        O: Into<Bytes> + 'static,
        E: Into<Box<dyn std::error::Error + Send + Sync>> + 'r,
        B: Into<String>,
    {
        let stream = stream.map_ok(Into::into).map_err(Into::into);
        Self::from_input(Input::Stream(Box::pin(stream)), boundary.into(), constraints)
    }

    fn from_input(input: Input<'r>, mut boundary: String, constraints: Constraints) -> Self {
        boundary.insert_str(0, "\r\n--");
        Multipart {
            state: Arc::new(Mutex::new(MultipartState {
                buffer: StreamBuffer::new(input, constraints.size_limit.whole_stream),
                marker: Finder::new(&boundary).into_owned(),
                stage: StreamingStage::FindingFirstBoundary(OpeningSearch::default()),
                next_field_idx: 0,
                curr_field_name: None,
                curr_field_size_limit: constraints.size_limit.per_field,
                curr_field_size_counter: 0,
                constraints,
            })),
        }
    }

    /// Construct a new `Multipart` instance with the given [`AsyncRead`] reader
    /// and the boundary.
    ///
    /// # Optional
    ///
    /// This requires the optional `tokio-io` feature to be enabled.
    ///
    /// # Examples
    ///
    /// ```
    /// use multer::Multipart;
    ///
    /// # async fn run() {
    /// let data =
    ///     "--X-BOUNDARY\r\nContent-Disposition: form-data; name=\"my_text_field\"\r\n\r\nabcd\r\n--X-BOUNDARY--\r\n";
    /// let reader = data.as_bytes();
    /// let mut multipart = Multipart::with_reader(reader, "X-BOUNDARY");
    ///
    /// while let Some(mut field) = multipart.next_field().await.unwrap() {
    ///     while let Some(chunk) = field.chunk().await.unwrap() {
    ///         println!("Chunk: {:?}", chunk);
    ///     }
    /// }
    /// # }
    /// # tokio::runtime::Runtime::new().unwrap().block_on(run());
    /// ```
    #[cfg(feature = "tokio-io")]
    #[cfg_attr(nightly, doc(cfg(feature = "tokio-io")))]
    pub fn with_reader<R, B>(reader: R, boundary: B) -> Self
    where
        R: AsyncRead + Unpin + Send + 'r,
        B: Into<String>,
    {
        Self::with_reader_with_constraints(reader, boundary, Constraints::default())
    }

    /// Construct a new `Multipart` instance with the given [`AsyncRead`] reader
    /// and the boundary.
    ///
    /// # Optional
    ///
    /// This requires the optional `tokio-io` feature to be enabled.
    ///
    /// # Examples
    ///
    /// ```
    /// use multer::Multipart;
    ///
    /// # async fn run() {
    /// let data =
    ///     "--X-BOUNDARY\r\nContent-Disposition: form-data; name=\"my_text_field\"\r\n\r\nabcd\r\n--X-BOUNDARY--\r\n";
    /// let reader = data.as_bytes();
    /// let mut multipart = Multipart::with_reader(reader, "X-BOUNDARY");
    ///
    /// while let Some(mut field) = multipart.next_field().await.unwrap() {
    ///     while let Some(chunk) = field.chunk().await.unwrap() {
    ///         println!("Chunk: {:?}", chunk);
    ///     }
    /// }
    /// # }
    /// # tokio::runtime::Runtime::new().unwrap().block_on(run());
    /// ```
    #[cfg(feature = "tokio-io")]
    #[cfg_attr(nightly, doc(cfg(feature = "tokio-io")))]
    pub fn with_reader_with_constraints<R, B>(reader: R, boundary: B, constraints: Constraints) -> Self
    where
        R: AsyncRead + Unpin + Send + 'r,
        B: Into<String>,
    {
        Self::from_input(Input::Reader(Box::new(reader)), boundary.into(), constraints)
    }

    /// Yields the next [`Field`] if available. After a terminal error has been
    /// reported by the parent, further calls return [`Error::MultipartFailed`].
    ///
    /// Any previous `Field` returned by this method must be dropped before
    /// calling this method or [`Multipart::next_field_with_idx()`] again. See
    /// [field-exclusivity](#field-exclusivity) for details.
    pub async fn next_field(&mut self) -> Result<Option<Field<'r>>> {
        future::poll_fn(|cx| self.poll_next_field(cx)).await
    }

    /// Yields the next [`Field`] if available. After a terminal error has been
    /// reported by the parent, further calls return [`Error::MultipartFailed`].
    ///
    /// Any previous `Field` returned by this method must be dropped before
    /// calling this method or [`Multipart::next_field_with_idx()`] again. See
    /// [field-exclusivity](#field-exclusivity) for details.
    ///
    /// This method is available since version 2.1.0.
    pub fn poll_next_field(&mut self, cx: &mut Context<'_>) -> Poll<Result<Option<Field<'r>>>> {
        // A live Field owns the only other reference, even after it has been exhausted.
        if Arc::strong_count(&self.state) != 1 {
            return Poll::Ready(Err(Error::LockFailure));
        }
        let mut state = self.state.try_lock().ok_or(Error::LockFailure)?;
        if let StreamingStage::Failed(report) = &mut state.stage {
            return Poll::Ready(Err(report.take().map_or(Error::MultipartFailed, |err| *err)));
        }

        let headers = match ready!(state.poll_headers(cx)) {
            Ok(Some(headers)) => headers,
            Ok(None) => return Poll::Ready(Ok(None)),
            Err(err) => {
                state.fail(None);
                return Poll::Ready(Err(err));
            }
        };

        let disposition = ContentDisposition::parse(&headers);
        let name = disposition.field_name.as_deref();
        state.curr_field_size_limit = state.constraints.size_limit.extract_size_limit_for(name);
        state.curr_field_name.clone_from(&disposition.field_name);
        state.curr_field_size_counter = 0;
        state.stage = StreamingStage::ReadingFieldData;
        if !state.constraints.is_it_allowed(name) {
            return Poll::Ready(Err(Error::UnknownField {
                field_name: disposition.field_name,
            }));
        }

        let idx = state.next_field_idx - 1;
        Poll::Ready(Ok(Some(Field::new(Arc::clone(&self.state), headers, idx, disposition))))
    }

    /// Yields the next [`Field`] with their positioning index as a tuple
    /// `(`[`usize`]`, `[`Field`]`)`.
    ///
    /// Any previous `Field` returned by this method must be dropped before
    /// calling this method or [`Multipart::next_field()`] again. See
    /// [field-exclusivity](#field-exclusivity) for details.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::convert::Infallible;
    ///
    /// use bytes::Bytes;
    /// use futures_util::stream::once;
    /// use multer::Multipart;
    ///
    /// # async fn run() {
    /// let data = "--X-BOUNDARY\r\nContent-Disposition: form-data; \
    ///     name=\"my_text_field\"\r\n\r\nabcd\r\n--X-BOUNDARY--\r\n";
    ///
    /// let stream = once(async move { Result::<Bytes, Infallible>::Ok(Bytes::from(data)) });
    /// let mut multipart = Multipart::new(stream, "X-BOUNDARY");
    ///
    /// while let Some((idx, field)) = multipart.next_field_with_idx().await.unwrap() {
    ///     println!("Index: {:?}, Content: {:?}", idx, field.text().await)
    /// }
    /// # }
    /// # tokio::runtime::Runtime::new().unwrap().block_on(run());
    /// ```
    pub async fn next_field_with_idx(&mut self) -> Result<Option<(usize, Field<'r>)>> {
        self.next_field().await.map(|f| f.map(|field| (field.index(), field)))
    }
}

impl MultipartState<'_> {
    fn poll_headers(&mut self, cx: &mut Context<'_>) -> Poll<Result<Option<HeaderMap>>> {
        let mut budget = REFILLS_PER_POLL;
        loop {
            let buf = &mut self.buffer.buf;
            match &mut self.stage {
                StreamingStage::FindingFirstBoundary(search) => {
                    if search.consume(buf, self.marker.needle()) {
                        self.stage = StreamingStage::DeterminingBoundaryType;
                        continue;
                    }
                }
                StreamingStage::ReadingFieldData => {
                    ready!(self.poll_body(cx, &mut budget)).map_err(|err| self.report(&err))?;
                    continue;
                }
                StreamingStage::DeterminingBoundaryType => {
                    if let Some(next) = buf.get(..2) {
                        if next == constants::BOUNDARY_EXT.as_bytes() {
                            self.stage = StreamingStage::Eof;
                            self.buffer.discard();
                            return Poll::Ready(Ok(None));
                        }
                        self.stage = StreamingStage::ReadingTransportPadding;
                        continue;
                    }
                }
                StreamingStage::ReadingTransportPadding => {
                    self.buffer.advance_past_transport_padding();
                    if let Some(next) = self.buffer.buf.get(..2) {
                        if next != constants::CRLF.as_bytes() {
                            return Poll::Ready(Err(Error::IncompleteStream));
                        }
                        self.buffer.buf.advance(2);
                        let limit = self.constraints.max_fields.unwrap_or(usize::MAX);
                        self.next_field_idx = self
                            .next_field_idx
                            .checked_add(1)
                            .filter(|&count| count <= limit)
                            .ok_or(Error::TooManyFields { limit })?;
                        self.stage = StreamingStage::ReadingFieldHeaders { scanned: 0 };
                        continue;
                    }
                }
                StreamingStage::ReadingFieldHeaders { scanned } => {
                    let terminator = constants::CRLF_CRLF.as_bytes();
                    let end = if buf.starts_with(constants::CRLF.as_bytes()) {
                        Some(2)
                    } else {
                        memchr::memmem::find(&buf[*scanned..], terminator).map(|pos| *scanned + pos + terminator.len())
                    };
                    let limit = self.constraints.size_limit.per_header;
                    if checked_size(0, end.unwrap_or(buf.len()), limit).is_none() {
                        return Poll::Ready(Err(Error::HeaderSizeExceeded { limit, header: None }));
                    }
                    if let Some(end) = end {
                        let mut slots = [httparse::EMPTY_HEADER; constants::MAX_HEADERS];
                        let raw =
                            match httparse::parse_headers(&buf[..end], &mut slots).map_err(Error::ReadHeaderFailed)? {
                                httparse::Status::Complete((_, headers)) => headers,
                                httparse::Status::Partial => return Poll::Ready(Err(Error::IncompleteHeaders)),
                            };
                        let headers = helpers::convert_raw_headers_to_header_map(raw)?;
                        for (name, value) in &headers {
                            if let Some(&limit) = self.constraints.size_limit.header_map.get(name) {
                                if checked_size(0, value.len(), limit).is_none() {
                                    return Poll::Ready(Err(Error::HeaderSizeExceeded {
                                        limit,
                                        header: Some((name.clone(), value.len())),
                                    }));
                                }
                            }
                        }
                        buf.advance(end);
                        return Poll::Ready(Ok(Some(headers)));
                    }
                    *scanned = buf.len().saturating_sub(terminator.len() - 1);
                }
                StreamingStage::Eof => return Poll::Ready(Ok(None)),
                StreamingStage::Failed(_) => unreachable!("caller checks for a terminal error"),
            }

            if self.buffer.eof {
                return Poll::Ready(Err(Error::IncompleteStream));
            }
            ready!(self.buffer.poll_refill(cx, &mut budget, None)).map_err(|err| self.report(&err))?;
        }
    }

    #[inline]
    pub(crate) fn poll_body(
        &mut self,
        cx: &mut Context<'_>,
        budget: &mut usize,
    ) -> Poll<Result<(bool, Bytes), ReadError>> {
        let (done, bytes) = ready!(self.buffer.poll_field_data(cx, budget, &self.marker))?;
        self.curr_field_size_counter =
            checked_size(self.curr_field_size_counter, bytes.len(), self.curr_field_size_limit)
                .ok_or(ReadError::FieldSize)?;
        if done {
            self.stage = StreamingStage::DeterminingBoundaryType;
        }
        Poll::Ready(Ok((done, bytes)))
    }

    fn report(&self, error: &ReadError) -> Error {
        match error {
            ReadError::Source(cause) => Error::StreamReadFailed(Arc::clone(cause)),
            ReadError::StreamSize => Error::StreamSizeExceeded {
                limit: self.constraints.size_limit.whole_stream,
            },
            ReadError::FieldSize => Error::FieldSizeExceeded {
                field_name: self.curr_field_name.clone(),
                limit: self.curr_field_size_limit,
            },
            ReadError::IncompleteField => Error::IncompleteFieldData {
                field_name: self.curr_field_name.clone(),
            },
        }
    }

    fn fail(&mut self, report: Option<Box<Error>>) {
        self.stage = StreamingStage::Failed(report);
        self.buffer.discard();
    }

    pub(crate) fn fail_field(&mut self, error: ReadError) -> Error {
        self.fail(Some(Box::new(self.report(&error))));
        self.report(&error)
    }
}

#[cfg(test)]
mod tests {
    use std::convert::Infallible;

    use futures_util::stream;

    use super::*;

    #[tokio::test]
    async fn count_overflow_is_terminal_even_with_default_limits() {
        let source = stream::iter([Ok::<_, Infallible>(Bytes::from_static(b"--B\r\n"))]);
        let mut m = Multipart::new(source, "B");
        m.state.lock().next_field_idx = usize::MAX;
        assert!(matches!(
            m.next_field().await,
            Err(Error::TooManyFields { limit: usize::MAX })
        ));
        assert!(matches!(m.next_field().await, Err(Error::MultipartFailed)));
    }

    #[tokio::test]
    async fn field_counter_checks_overflow_with_a_buffered_close() {
        for skip in [false, true] {
            let source = stream::iter([Ok::<_, Infallible>(Bytes::from_static(b"--B\r\n\r\nx\r\n--B--"))]);
            let mut m = Multipart::new(source, "B");
            let mut field = m.next_field().await.unwrap().unwrap();
            m.state.lock().curr_field_size_counter = u64::MAX;
            if !skip {
                assert!(matches!(
                    field.chunk().await,
                    Err(Error::FieldSizeExceeded {
                        limit: u64::MAX,
                        field_name: None
                    })
                ));
            }
            drop(field);
            assert!(matches!(
                m.next_field().await,
                Err(Error::FieldSizeExceeded {
                    limit: u64::MAX,
                    field_name: None
                })
            ));
            assert!(matches!(m.next_field().await, Err(Error::MultipartFailed)));
        }
    }
}
