use crate::state::{MultipartState, StreamingStage};
use crate::{constants, ErrorExt};
use bytes::{Bytes, BytesMut};
use encoding_rs::{Encoding, UTF_8};
use futures::stream::{Stream, TryStreamExt};
use http::header::{self, HeaderMap};
#[cfg(feature = "json")]
use serde::de::DeserializeOwned;
#[cfg(feature = "json")]
use serde_json;
use std::borrow::Cow;
use std::ops::DerefMut;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

pub struct Field<S: Stream<Item = Result<Bytes, crate::Error>> + Send + Sync + Unpin + 'static> {
    state: Arc<Mutex<MultipartState<S>>>,
    headers: HeaderMap,
    done: bool,
    meta: FieldMeta,
}

struct FieldMeta {
    name: Option<String>,
    file_name: Option<String>,
    content_type: Option<mime::Mime>,
    idx: usize,
}

impl<S: Stream<Item = Result<Bytes, crate::Error>> + Send + Sync + Unpin + 'static> Field<S> {
    pub(crate) fn new(state: Arc<Mutex<MultipartState<S>>>, headers: HeaderMap, idx: usize) -> Self {
        let (name, file_name) = Self::parse_content_disposition(&headers);
        let content_type = Self::parse_content_type(&headers);

        Field {
            state,
            headers,
            done: false,
            meta: FieldMeta {
                name,
                file_name,
                content_type,
                idx,
            },
        }
    }

    fn parse_content_disposition(headers: &HeaderMap) -> (Option<String>, Option<String>) {
        let content_disposition = headers
            .get(header::CONTENT_DISPOSITION)
            .and_then(|val| val.to_str().ok());

        let name = content_disposition
            .and_then(|val| constants::CONTENT_DISPOSITION_FIELD_NAME_RE.captures(val))
            .and_then(|cap| cap.get(1))
            .map(|m| m.as_str().to_owned());

        let file_name = content_disposition
            .and_then(|val| constants::CONTENT_DISPOSITION_FILE_NAME_RE.captures(val))
            .and_then(|cap| cap.get(1))
            .map(|m| m.as_str().to_owned());

        (name, file_name)
    }

    fn parse_content_type(headers: &HeaderMap) -> Option<mime::Mime> {
        headers
            .get(header::CONTENT_TYPE)
            .and_then(|val| val.to_str().ok())
            .and_then(|val| val.parse::<mime::Mime>().ok())
    }

    pub fn name(&self) -> Option<&str> {
        self.meta.name.as_ref().map(|name| name.as_str())
    }

    pub fn file_name(&self) -> Option<&str> {
        self.meta.file_name.as_ref().map(|file_name| file_name.as_str())
    }

    pub fn content_type(&self) -> Option<&mime::Mime> {
        self.meta.content_type.as_ref()
    }

    pub fn headers(&self) -> &HeaderMap {
        &self.headers
    }

    pub async fn bytes(mut self) -> crate::Result<Bytes> {
        let mut buf = BytesMut::new();

        while let Some(bytes) = self.chunk().await? {
            buf.extend_from_slice(&bytes);
        }

        Ok(buf.freeze())
    }

    pub async fn chunk(&mut self) -> crate::Result<Option<Bytes>> {
        self.try_next().await
    }

    #[cfg(feature = "json")]
    pub async fn json<T: DeserializeOwned>(self) -> crate::Result<T> {
        self.bytes()
            .await
            .context("Couldn't read field data as `Bytes`")
            .and_then(|bytes| serde_json::from_slice(&bytes).context("Couldn't parse field data as JSON"))
    }

    pub async fn text(self) -> crate::Result<String> {
        self.text_with_charset("utf-8").await
    }

    pub async fn text_with_charset(self, default_encoding: &str) -> crate::Result<String> {
        let encoding_name = self
            .content_type()
            .and_then(|mime| mime.get_param(mime::CHARSET))
            .map(|charset| charset.as_str())
            .unwrap_or(default_encoding);

        let encoding = Encoding::for_label(encoding_name.as_bytes()).unwrap_or(UTF_8);

        let bytes = self.bytes().await?;

        let (text, _, _) = encoding.decode(&bytes);

        match text {
            Cow::Owned(s) => Ok(s),
            Cow::Borrowed(s) => Ok(String::from(s)),
        }
    }

    pub fn index(&self) -> usize {
        self.meta.idx
    }
}

impl<S: Stream<Item = Result<Bytes, crate::Error>> + Send + Sync + Unpin + 'static> Stream for Field<S> {
    type Item = Result<Bytes, crate::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        if self.done {
            return Poll::Ready(None);
        }

        let mut mutex_guard = match self.state.lock() {
            Ok(lock) => lock,
            Err(err) => {
                return Poll::Ready(Some(Err(
                    crate::Error::new(err.to_string()).context("Couldn't lock the multipart state")
                )));
            }
        };

        let state: &mut MultipartState<S> = mutex_guard.deref_mut();

        let stream_buffer = &mut state.buffer;

        if let Err(err) = stream_buffer.poll_stream(cx) {
            return Poll::Ready(Some(Err(err.context("Couldn't read data from the stream"))));
        }

        match stream_buffer.read_field_data(state.boundary.as_str()) {
            Ok(Some((true, bytes))) => {
                drop(mutex_guard);

                self.done = true;

                Poll::Ready(Some(Ok(bytes)))
            }
            Ok(Some((false, bytes))) => Poll::Ready(Some(Ok(bytes))),
            Ok(None) => Poll::Pending,
            Err(err) => Poll::Ready(Some(Err(err))),
        }
    }
}

impl<S: Stream<Item = Result<Bytes, crate::Error>> + Send + Sync + Unpin + 'static> Drop for Field<S> {
    fn drop(&mut self) {
        let mut mutex_guard = match self.state.lock() {
            Ok(lock) => lock,
            Err(err) => {
                log::error!(
                    "{}",
                    crate::Error::new(err.to_string()).context("Couldn't lock the multipart state")
                );
                return;
            }
        };

        let state: &mut MultipartState<S> = mutex_guard.deref_mut();

        if self.done {
            state.stage = StreamingStage::ReadingBoundary;
        } else {
            state.stage = StreamingStage::CleaningPrevFieldData;
        }

        state.is_prev_field_consumed = true;

        if let Some(waker) = state.next_field_waker.take() {
            waker.clone().wake();
        }
    }
}
