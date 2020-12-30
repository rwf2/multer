use crate::buffer::StreamBuffer;
use crate::constants;
use crate::constraints::Constraints;
use crate::content_disposition::ContentDisposition;
use crate::helpers;
use crate::state::{MultipartState, StreamingStage};
use crate::Field;
use bytes::Bytes;
use futures::stream::{Stream, TryStreamExt};
use std::ops::DerefMut;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
#[cfg(feature = "reader")]
use tokio::io::AsyncRead;
#[cfg(feature = "reader")]
use tokio_util::io::ReaderStream;

/// Represents the implementation of `multipart/form-data` formatted data.
///
/// This will parse the source stream into [`Field`](./struct.Field.html) instances via its [`Stream`](https://docs.rs/futures/0.3.5/futures/stream/trait.Stream.html)
/// implementation.
///
/// To maintain consistency in the underlying stream, this will not yield more than one [`Field`](./struct.Field.html) at a time.
/// A [`Drop`](https://doc.rust-lang.org/nightly/std/ops/trait.Drop.html) implementation on [`Field`](./struct.Field.html) is used to signal
/// when it's time to move forward, so do avoid leaking that type or anything which contains it.
///
/// The Fields can be accessed via the [`Stream`](./struct.Multipart.html#impl-Stream) API or the methods defined in this type.
///
/// # Examples
///
/// ```
/// use multer::Multipart;
/// use bytes::Bytes;
/// use std::convert::Infallible;
/// use futures::stream::once;
///
/// # async fn run() {
/// let data = "--X-BOUNDARY\r\nContent-Disposition: form-data; name=\"my_text_field\"\r\n\r\nabcd\r\n--X-BOUNDARY--\r\n";
/// let stream = once(async move { Result::<Bytes, Infallible>::Ok(Bytes::from(data)) });
/// let mut multipart = Multipart::new(stream, "X-BOUNDARY");
///
/// while let Some(field) = multipart.next_field().await.unwrap() {
///     println!("Field: {:?}", field.text().await)
/// }
/// # }
/// # tokio::runtime::Runtime::new().unwrap().block_on(run());
/// ```
pub struct Multipart {
    state: Arc<Mutex<MultipartState>>,
    constraints: Constraints,
}

impl Multipart {
    /// Construct a new `Multipart` instance with the given [`Bytes`](https://docs.rs/bytes/0.5.4/bytes/struct.Bytes.html) stream and the boundary.
    pub fn new<S, O, E, B>(stream: S, boundary: B) -> Multipart
    where
        S: Stream<Item = Result<O, E>> + Send + 'static,
        O: Into<Bytes> + 'static,
        E: Into<Box<dyn std::error::Error + Send + Sync>> + 'static,
        B: Into<String>,
    {
        let constraints = Constraints::default();

        let stream = stream
            .map_ok(|b| b.into())
            .map_err(|err| crate::Error::StreamReadFailed(err.into()));

        let state = MultipartState {
            buffer: StreamBuffer::new(stream, constraints.size_limit.whole_stream),
            boundary: boundary.into(),
            stage: StreamingStage::ReadingBoundary,
            is_prev_field_consumed: true,
            next_field_waker: None,
            next_field_idx: 0,
            curr_field_name: None,
            curr_field_size_limit: constraints.size_limit.per_field,
            curr_field_size_counter: 0,
        };

        Multipart {
            state: Arc::new(Mutex::new(state)),
            constraints,
        }
    }

    /// Construct a new `Multipart` instance with the given [`Bytes`](https://docs.rs/bytes/0.5.4/bytes/struct.Bytes.html) stream and the boundary.
    pub fn new_with_constraints<S, O, E, B>(stream: S, boundary: B, constraints: Constraints) -> Multipart
    where
        S: Stream<Item = Result<O, E>> + Send + 'static,
        O: Into<Bytes> + 'static,
        E: Into<Box<dyn std::error::Error + Send + Sync>> + 'static,
        B: Into<String>,
    {
        let stream = stream
            .map_ok(|b| b.into())
            .map_err(|err| crate::Error::StreamReadFailed(err.into()));

        let state = MultipartState {
            buffer: StreamBuffer::new(stream, constraints.size_limit.whole_stream),
            boundary: boundary.into(),
            stage: StreamingStage::ReadingBoundary,
            is_prev_field_consumed: true,
            next_field_waker: None,
            next_field_idx: 0,
            curr_field_name: None,
            curr_field_size_limit: constraints.size_limit.per_field,
            curr_field_size_counter: 0,
        };

        Multipart {
            state: Arc::new(Mutex::new(state)),
            constraints,
        }
    }

    /// Construct a new `Multipart` instance with the given [`AsyncRead`](https://docs.rs/tokio/0.2.20/tokio/io/trait.AsyncRead.html) reader and the boundary.
    ///
    /// # Optional
    ///
    /// This requires the optional `reader` feature to be enabled.
    ///
    /// # Examples
    ///
    /// ```
    /// use multer::Multipart;
    /// use bytes::Bytes;
    /// use std::convert::Infallible;
    /// use futures::stream::once;
    ///
    /// # async fn run() {
    /// let data = "--X-BOUNDARY\r\nContent-Disposition: form-data; name=\"my_text_field\"\r\n\r\nabcd\r\n--X-BOUNDARY--\r\n";
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
    #[cfg(feature = "reader")]
    pub fn with_reader<R, B>(reader: R, boundary: B) -> Multipart
    where
        R: AsyncRead + Send + 'static,
        B: Into<String>,
    {
        let stream = ReaderStream::new(reader);
        Multipart::new(stream, boundary)
    }

    /// Construct a new `Multipart` instance with the given [`AsyncRead`](https://docs.rs/tokio/0.2.20/tokio/io/trait.AsyncRead.html) reader and the boundary.
    ///
    /// # Optional
    ///
    /// This requires the optional `reader` feature to be enabled.
    ///
    /// # Examples
    ///
    /// ```
    /// use multer::Multipart;
    /// use bytes::Bytes;
    /// use std::convert::Infallible;
    /// use futures::stream::once;
    ///
    /// # async fn run() {
    /// let data = "--X-BOUNDARY\r\nContent-Disposition: form-data; name=\"my_text_field\"\r\n\r\nabcd\r\n--X-BOUNDARY--\r\n";
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
    #[cfg(feature = "reader")]
    pub fn with_reader_with_constraints<R, B>(reader: R, boundary: B, constraints: Constraints) -> Multipart
    where
        R: AsyncRead + Send + 'static,
        B: Into<String>,
    {
        let stream = ReaderStream::new(reader);
        Multipart::new_with_constraints(stream, boundary, constraints)
    }

    /// Yields the next [`Field`](./struct.Field.html) if available.
    ///
    /// For more info, go to [`Field`](./struct.Field.html#warning-about-leaks).
    pub async fn next_field(&mut self) -> crate::Result<Option<Field>> {
        self.try_next().await
    }

    /// Yields the next [`Field`](./struct.Field.html) with their positioning index as a tuple `(usize, Field)`.
    ///
    /// For more info, go to [`Field`](./struct.Field.html#warning-about-leaks).
    ///
    /// # Examples
    ///
    /// ```
    /// use multer::Multipart;
    /// use bytes::Bytes;
    /// use std::convert::Infallible;
    /// use futures::stream::once;
    ///
    /// # async fn run() {
    /// let data = "--X-BOUNDARY\r\nContent-Disposition: form-data; name=\"my_text_field\"\r\n\r\nabcd\r\n--X-BOUNDARY--\r\n";
    /// let reader = data.as_bytes();
    /// let mut multipart = Multipart::with_reader(reader, "X-BOUNDARY");
    ///
    /// while let Some((idx, field)) = multipart.next_field_with_idx().await.unwrap() {
    ///     println!("Index: {:?}, Content: {:?}", idx, field.text().await)
    /// }
    /// # }
    /// # tokio::runtime::Runtime::new().unwrap().block_on(run());
    /// ```
    pub async fn next_field_with_idx(&mut self) -> crate::Result<Option<(usize, Field)>> {
        self.try_next().await.map(|f| f.map(|field| (field.index(), field)))
    }
}

impl Stream for Multipart {
    type Item = Result<Field, crate::Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        let mut mutex_guard = match self.state.lock() {
            Ok(lock) => lock,
            Err(err) => {
                return Poll::Ready(Some(Err(crate::Error::LockFailure(err.to_string().into()))));
            }
        };

        let state: &mut MultipartState = mutex_guard.deref_mut();

        if state.stage == StreamingStage::Eof {
            return Poll::Ready(None);
        }

        if !state.is_prev_field_consumed {
            state.next_field_waker = Some(cx.waker().clone());
            return Poll::Pending;
        }

        let stream_buffer = &mut state.buffer;

        if let Err(err) = stream_buffer.poll_stream(cx) {
            return Poll::Ready(Some(Err(crate::Error::StreamReadFailed(err.into()))));
        }

        if state.stage == StreamingStage::CleaningPrevFieldData {
            match stream_buffer.read_field_data(state.boundary.as_str(), state.curr_field_name.as_deref()) {
                Ok(Some((done, bytes))) => {
                    state.curr_field_size_counter += bytes.len() as u64;

                    if state.curr_field_size_counter > state.curr_field_size_limit {
                        return Poll::Ready(Some(Err(crate::Error::FieldSizeExceeded {
                            limit: state.curr_field_size_limit,
                            field_name: state.curr_field_name.clone(),
                        })));
                    }

                    if done {
                        state.stage = StreamingStage::ReadingBoundary;
                    } else {
                        return Poll::Pending;
                    }
                }
                Ok(None) => {
                    return Poll::Pending;
                }
                Err(err) => {
                    return Poll::Ready(Some(Err(err)));
                }
            }
        }

        if state.stage == StreamingStage::ReadingBoundary {
            let boundary = &state.boundary;
            let boundary_deriv_len = constants::BOUNDARY_EXT.len() + boundary.len() + 2;

            let boundary_bytes = match stream_buffer.read_exact(boundary_deriv_len) {
                Some(bytes) => bytes,
                None => {
                    return if stream_buffer.eof {
                        Poll::Ready(Some(Err(crate::Error::IncompleteStream)))
                    } else {
                        Poll::Pending
                    };
                }
            };

            if &boundary_bytes[..]
                == format!("{}{}{}", constants::BOUNDARY_EXT, boundary, constants::BOUNDARY_EXT).as_bytes()
            {
                state.stage = StreamingStage::Eof;
                return Poll::Ready(None);
            }

            if &boundary_bytes[..] != format!("{}{}{}", constants::BOUNDARY_EXT, boundary, constants::CRLF).as_bytes() {
                return Poll::Ready(Some(Err(crate::Error::IncompleteStream)));
            } else {
                state.stage = StreamingStage::ReadingFieldHeaders;
            }
        }

        if state.stage == StreamingStage::ReadingFieldHeaders {
            let header_bytes = match stream_buffer.read_until(constants::CRLF_CRLF.as_bytes()) {
                Some(bytes) => bytes,
                None => {
                    return if stream_buffer.eof {
                        return Poll::Ready(Some(Err(crate::Error::IncompleteStream)));
                    } else {
                        Poll::Pending
                    };
                }
            };

            let mut headers = [httparse::EMPTY_HEADER; constants::MAX_HEADERS];

            let headers = match httparse::parse_headers(&header_bytes, &mut headers) {
                Ok(httparse::Status::Complete((_, raw_headers))) => {
                    match helpers::convert_raw_headers_to_header_map(raw_headers) {
                        Ok(headers) => headers,
                        Err(err) => {
                            return Poll::Ready(Some(Err(err)));
                        }
                    }
                }
                Ok(httparse::Status::Partial) => {
                    return Poll::Ready(Some(Err(crate::Error::IncompleteHeaders)));
                }
                Err(err) => {
                    return Poll::Ready(Some(Err(crate::Error::ReadHeaderFailed(err.into()))));
                }
            };

            state.stage = StreamingStage::ReadingFieldData;
            state.is_prev_field_consumed = false;

            let field_idx = state.next_field_idx;
            state.next_field_idx += 1;

            let content_disposition = ContentDisposition::parse(&headers);
            let field_size_limit = self
                .constraints
                .size_limit
                .extract_size_limit_for(content_disposition.field_name.as_deref());

            state.curr_field_name = content_disposition.field_name.clone();
            state.curr_field_size_limit = field_size_limit;
            state.curr_field_size_counter = 0;

            drop(mutex_guard);

            let next_field = Field::new(Arc::clone(&self.state), headers, field_idx, content_disposition);
            let field_name = next_field.name().map(|name| name.to_owned());

            if !self.constraints.is_it_allowed(field_name.as_deref()) {
                return Poll::Ready(Some(Err(crate::Error::UnknownField {
                    field_name: field_name.clone(),
                })));
            }

            return Poll::Ready(Some(Ok(next_field)));
        }

        state.next_field_waker = Some(cx.waker().clone());
        Poll::Pending
    }
}
