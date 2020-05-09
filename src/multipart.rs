use crate::buffer::StreamBuffer;
use crate::constants;
use crate::helpers;
use crate::state::{MultipartState, StreamingStage};
use crate::{ErrorExt, Field};
use bytes::Bytes;
use futures::stream::{Stream, TryStreamExt};
use std::ops::DerefMut;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

/// The previous field must be consumed, (dropping a field will not work) to get the next field.
pub struct Multipart<S> {
    state: Arc<Mutex<MultipartState<S>>>,
}

impl<S: Stream<Item = Result<Bytes, crate::Error>> + Send + Sync + Unpin + 'static> Multipart<S> {
    pub fn new<B: Into<String>>(stream: S, boundary: B) -> Multipart<S> {
        let state = MultipartState {
            buffer: StreamBuffer::new(stream),
            boundary: boundary.into(),
            stage: StreamingStage::ReadingBoundary,
            is_prev_field_consumed: true,
            next_field_waker: None,
            next_field_idx: 0,
        };

        Multipart {
            state: Arc::new(Mutex::new(state)),
        }
    }

    pub async fn next_field(&mut self) -> crate::Result<Option<Field<S>>> {
        self.try_next().await
    }

    pub async fn next_field_with_idx(&mut self) -> crate::Result<Option<(usize, Field<S>)>> {
        self.try_next().await.map(|f| f.map(|field| (field.index(), field)))
    }
}

impl<S: Stream<Item = Result<Bytes, crate::Error>> + Send + Sync + Unpin + 'static> Stream for Multipart<S> {
    type Item = Result<Field<S>, crate::Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        let mut mutex_guard = match self.state.lock() {
            Ok(lock) => lock,
            Err(err) => {
                return Poll::Ready(Some(Err(
                    crate::Error::new(err.to_string()).context("Couldn't lock the multipart state")
                )));
            }
        };

        let state: &mut MultipartState<S> = mutex_guard.deref_mut();

        if state.stage == StreamingStage::Eof {
            return Poll::Ready(None);
        }

        if !state.is_prev_field_consumed {
            state.next_field_waker = Some(cx.waker().clone());
            return Poll::Pending;
        }

        let stream_buffer = &mut state.buffer;

        if let Err(err) = stream_buffer.poll_stream(cx) {
            return Poll::Ready(Some(Err(err.context("Couldn't read data from the stream"))));
        }

        if state.stage == StreamingStage::CleaningPrevFieldData {
            match stream_buffer.read_field_data(state.boundary.as_str()) {
                Ok(Some((true, _))) => {
                    state.stage = StreamingStage::ReadingBoundary;
                }
                Ok(Some((false, _))) => {
                    return Poll::Pending;
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
                        Poll::Ready(Some(Err(crate::Error::new(
                            "Incomplete stream, couldn't read the boundary",
                        ))))
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
                return Poll::Ready(Some(Err(crate::Error::new(
                    "The stream is not valid multipart/form-data",
                ))));
            } else {
                state.stage = StreamingStage::ReadingFieldHeaders;
            }
        }

        if state.stage == StreamingStage::ReadingFieldHeaders {
            let header_bytes = match stream_buffer.read_until(constants::CRLF_CRLF.as_bytes()) {
                Some(bytes) => bytes,
                None => {
                    return if stream_buffer.eof {
                        Poll::Ready(Some(Err(crate::Error::new(
                            "Incomplete stream, couldn't read the field headers",
                        ))))
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
                    return Poll::Ready(Some(Err(crate::Error::new(
                        "Incomplete headers, couldn't read the field headers completely",
                    ))));
                }
                Err(err) => {
                    return Poll::Ready(Some(Err(err.context("Failed to read the field headers"))));
                }
            };

            state.stage = StreamingStage::ReadingFieldData;
            state.is_prev_field_consumed = false;

            let field_idx = state.next_field_idx;
            state.next_field_idx += 1;

            drop(mutex_guard);

            let next_field = Field::new(Arc::clone(&self.state), headers, field_idx);
            return Poll::Ready(Some(Ok(next_field)));
        }

        state.next_field_waker = Some(cx.waker().clone());
        Poll::Pending
    }
}
