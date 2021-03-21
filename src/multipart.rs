use std::task::{Context, Poll};

use bytes::Bytes;
use futures_util::future;
use futures_util::stream::{Stream, TryStreamExt};
#[cfg(feature = "tokio-io")]
use {tokio::io::AsyncRead, tokio_util::io::ReaderStream};

use crate::buffer::StreamBuffer;
use crate::constants;
use crate::constraints::Constraints;
use crate::content_disposition::ContentDisposition;
use crate::field::{Field, FieldData};
use crate::helpers;
use crate::state::{MultipartState, StreamingStage};

/// Represents the implementation of `multipart/form-data` formatted data.
///
/// This will parse the source stream into [`Field`] instances via
/// [`next_field`](Self::next_field).
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
/// let data =
///     "--X-BOUNDARY\r\nContent-Disposition: form-data; name=\"my_text_field\"\r\n\r\nabcd\r\n--X-BOUNDARY--\r\n";
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
pub struct Multipart {
    state: MultipartState,
    constraints: Constraints,
}

impl Multipart {
    /// Construct a new `Multipart` instance with the given [`Bytes`] stream and
    /// the boundary.
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
            stage: StreamingStage::FindingFirstBoundary,
            next_field_idx: 0,
            curr_field_name: None,
            curr_field_size_limit: constraints.size_limit.per_field,
            curr_field_size_counter: 0,
        };

        Multipart { state, constraints }
    }

    /// Construct a new `Multipart` instance with the given [`Bytes`] stream and
    /// the boundary.
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
            stage: StreamingStage::FindingFirstBoundary,
            next_field_idx: 0,
            curr_field_name: None,
            curr_field_size_limit: constraints.size_limit.per_field,
            curr_field_size_counter: 0,
        };

        Multipart { state, constraints }
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
    pub fn with_reader<R, B>(reader: R, boundary: B) -> Multipart
    where
        R: AsyncRead + Unpin + Send + 'static,
        B: Into<String>,
    {
        let stream = ReaderStream::new(reader);
        Multipart::new(stream, boundary)
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
    pub fn with_reader_with_constraints<R, B>(reader: R, boundary: B, constraints: Constraints) -> Multipart
    where
        R: AsyncRead + Unpin + Send + 'static,
        B: Into<String>,
    {
        let stream = ReaderStream::new(reader);
        Multipart::new_with_constraints(stream, boundary, constraints)
    }

    /// Yields the next [`Field`] if available.
    pub async fn next_field(&mut self) -> crate::Result<Option<Field<'_>>> {
        let data = future::poll_fn(|cx| self.poll_next_field(cx)).await?;
        Ok(data.map(move |data| Field::from_data(&mut self.state, data)))
    }

    fn poll_next_field(&mut self, cx: &mut Context<'_>) -> Poll<crate::Result<Option<FieldData>>> {
        if self.state.stage == StreamingStage::Eof {
            return Poll::Ready(Ok(None));
        }

        let stream_buffer = &mut self.state.buffer;

        if let Err(err) = stream_buffer.poll_stream(cx) {
            return Poll::Ready(Err(crate::Error::StreamReadFailed(err.into())));
        }

        if self.state.stage == StreamingStage::FindingFirstBoundary {
            let boundary = &self.state.boundary;
            let boundary_deriv = format!("{}{}", constants::BOUNDARY_EXT, boundary);
            match stream_buffer.read_to(boundary_deriv.as_bytes()) {
                Some(_) => self.state.stage = StreamingStage::ReadingBoundary,
                None => {
                    if let Err(err) = stream_buffer.poll_stream(cx) {
                        return Poll::Ready(Err(crate::Error::StreamReadFailed(err.into())));
                    }
                    if stream_buffer.eof {
                        return Poll::Ready(Err(crate::Error::IncompleteStream));
                    }
                }
            }
        }

        // The previous field did not finish reading its data.
        if self.state.stage == StreamingStage::ReadingFieldData {
            match stream_buffer.read_field_data(self.state.boundary.as_str(), self.state.curr_field_name.as_deref())? {
                Some((done, bytes)) => {
                    self.state.curr_field_size_counter += bytes.len() as u64;

                    if self.state.curr_field_size_counter > self.state.curr_field_size_limit {
                        return Poll::Ready(Err(crate::Error::FieldSizeExceeded {
                            limit: self.state.curr_field_size_limit,
                            field_name: self.state.curr_field_name.clone(),
                        }));
                    }

                    if done {
                        self.state.stage = StreamingStage::ReadingBoundary;
                    } else {
                        return Poll::Pending;
                    }
                }
                None => {
                    return Poll::Pending;
                }
            }
        }

        if self.state.stage == StreamingStage::ReadingBoundary {
            let boundary = &self.state.boundary;
            let boundary_deriv_len = constants::BOUNDARY_EXT.len() + boundary.len();

            let boundary_bytes = match stream_buffer.read_exact(boundary_deriv_len) {
                Some(bytes) => bytes,
                None => {
                    return if stream_buffer.eof {
                        Poll::Ready(Err(crate::Error::IncompleteStream))
                    } else {
                        Poll::Pending
                    };
                }
            };

            if &boundary_bytes[..] == format!("{}{}", constants::BOUNDARY_EXT, boundary).as_bytes() {
                self.state.stage = StreamingStage::DeterminingBoundaryType;
            } else {
                return Poll::Ready(Err(crate::Error::IncompleteStream));
            }
        }

        if self.state.stage == StreamingStage::DeterminingBoundaryType {
            let ext_len = constants::BOUNDARY_EXT.len();
            let next_bytes = match stream_buffer.peek_exact(ext_len) {
                Some(bytes) => bytes,
                None => {
                    return if stream_buffer.eof {
                        Poll::Ready(Err(crate::Error::IncompleteStream))
                    } else {
                        Poll::Pending
                    };
                }
            };

            if next_bytes == constants::BOUNDARY_EXT.as_bytes() {
                self.state.stage = StreamingStage::Eof;
                return Poll::Ready(Ok(None));
            } else {
                self.state.stage = StreamingStage::ReadingTransportPadding;
            }
        }

        if self.state.stage == StreamingStage::ReadingTransportPadding {
            if !stream_buffer.advance_past_transport_padding() {
                return if stream_buffer.eof {
                    Poll::Ready(Err(crate::Error::IncompleteStream))
                } else {
                    Poll::Pending
                };
            }

            let crlf_len = constants::CRLF.len();
            let crlf_bytes = match stream_buffer.read_exact(crlf_len) {
                Some(bytes) => bytes,
                None => {
                    return if stream_buffer.eof {
                        Poll::Ready(Err(crate::Error::IncompleteStream))
                    } else {
                        Poll::Pending
                    };
                }
            };

            if &crlf_bytes[..] == constants::CRLF.as_bytes() {
                self.state.stage = StreamingStage::ReadingFieldHeaders;
            } else {
                return Poll::Ready(Err(crate::Error::IncompleteStream));
            }
        }

        if self.state.stage == StreamingStage::ReadingFieldHeaders {
            let header_bytes = match stream_buffer.read_until(constants::CRLF_CRLF.as_bytes()) {
                Some(bytes) => bytes,
                None => {
                    return if stream_buffer.eof {
                        return Poll::Ready(Err(crate::Error::IncompleteStream));
                    } else {
                        Poll::Pending
                    };
                }
            };

            let mut headers = [httparse::EMPTY_HEADER; constants::MAX_HEADERS];

            let headers =
                match httparse::parse_headers(&header_bytes, &mut headers).map_err(crate::Error::ReadHeaderFailed)? {
                    httparse::Status::Complete((_, raw_headers)) => {
                        match helpers::convert_raw_headers_to_header_map(raw_headers) {
                            Ok(headers) => headers,
                            Err(err) => {
                                return Poll::Ready(Err(err));
                            }
                        }
                    }
                    httparse::Status::Partial => {
                        return Poll::Ready(Err(crate::Error::IncompleteHeaders));
                    }
                };

            self.state.stage = StreamingStage::ReadingFieldData;

            let field_idx = self.state.next_field_idx;
            self.state.next_field_idx += 1;

            let content_disposition = ContentDisposition::parse(&headers);
            let field_size_limit = self
                .constraints
                .size_limit
                .extract_size_limit_for(content_disposition.field_name.as_deref());

            self.state.curr_field_name = content_disposition.field_name.clone();
            self.state.curr_field_size_limit = field_size_limit;
            self.state.curr_field_size_counter = 0;

            let next_field = FieldData::new(headers, field_idx, content_disposition);

            if !self.constraints.is_it_allowed(next_field.name()) {
                return Poll::Ready(Err(crate::Error::UnknownField {
                    field_name: next_field.name().map(str::to_owned),
                }));
            }

            return Poll::Ready(Ok(Some(next_field)));
        }

        Poll::Pending
    }

    /// Yields the next [`Field`] with their positioning index as a tuple
    /// `(`[`usize`]`, `[`Field`]`)`.
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
    /// let data =
    ///     "--X-BOUNDARY\r\nContent-Disposition: form-data; name=\"my_text_field\"\r\n\r\nabcd\r\n--X-BOUNDARY--\r\n";
    /// let stream = once(async move { Result::<Bytes, Infallible>::Ok(Bytes::from(data)) });
    /// let mut multipart = Multipart::new(stream, "X-BOUNDARY");
    ///
    /// while let Some((idx, field)) = multipart.next_field_with_idx().await.unwrap() {
    ///     println!("Index: {:?}, Content: {:?}", idx, field.text().await)
    /// }
    /// # }
    /// # tokio::runtime::Runtime::new().unwrap().block_on(run());
    /// ```
    pub async fn next_field_with_idx(&mut self) -> crate::Result<Option<(usize, Field<'_>)>> {
        self.next_field().await.map(|f| f.map(|field| (field.index(), field)))
    }
}
