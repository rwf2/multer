use std::borrow::Cow;
use std::ops::DerefMut;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use bytes::{Bytes, BytesMut};
use encoding_rs::{Encoding, UTF_8};
use futures_util::stream::{Stream, TryStreamExt};
use http::header::HeaderMap;
#[cfg(feature = "json")]
use serde::de::DeserializeOwned;

use crate::content_disposition::ContentDisposition;
use crate::helpers;
use crate::state::{MultipartState, StreamingStage};

/// A single field in a multipart stream.
///
/// Its content can be accessed via the [`Stream`] API or the methods defined in
/// this type.
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
///     let content = field.text().await.unwrap();
///     assert_eq!(content, "abcd");
/// }
/// # }
/// # tokio::runtime::Runtime::new().unwrap().block_on(run());
/// ```
///
/// ## Warning About Leaks
///
/// To avoid the next field being initialized before this one is done being read
/// or dropped, only one instance per [`Multipart`] instance is allowed at a
/// time. A [`Drop`] implementation is used to notify [`Multipart`] that this
/// field is done being read.
///
/// If this value is leaked (via [`std::mem::forget()`] or some other
/// mechanism), then the parent [`Multipart`] will never be able to yield the
/// next field in the stream. The task waiting on the [`Multipart`] will also
/// never be notified, which, depending on the executor implementation,
/// may cause a deadlock.
///
/// [`Multipart`]: crate::Multipart
#[derive(Debug)]
pub struct Field {
    state: Arc<Mutex<MultipartState>>,
    headers: HeaderMap,
    done: bool,
    meta: FieldMeta,
}

#[derive(Debug)]
struct FieldMeta {
    content_disposition: ContentDisposition,
    content_type: Option<mime::Mime>,
    idx: usize,
}

impl Field {
    pub(crate) fn new(
        state: Arc<Mutex<MultipartState>>,
        headers: HeaderMap,
        idx: usize,
        content_disposition: ContentDisposition,
    ) -> Self {
        let content_type = helpers::parse_content_type(&headers);

        Field {
            state,
            headers,
            done: false,
            meta: FieldMeta {
                content_disposition,
                content_type,
                idx,
            },
        }
    }

    /// The field name found in the [`Content-Disposition`](https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Content-Disposition) header.
    pub fn name(&self) -> Option<&str> {
        self.meta.content_disposition.field_name.as_deref()
    }

    /// The file name found in the [`Content-Disposition`](https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Content-Disposition) header.
    pub fn file_name(&self) -> Option<&str> {
        self.meta.content_disposition.file_name.as_deref()
    }

    /// Get the content type of the field.
    pub fn content_type(&self) -> Option<&mime::Mime> {
        self.meta.content_type.as_ref()
    }

    /// Get a map of headers as [`HeaderMap`].
    pub fn headers(&self) -> &HeaderMap {
        &self.headers
    }

    /// Get the full data of the field as [`Bytes`].
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
    ///     let bytes = field.bytes().await.unwrap();
    ///     assert_eq!(bytes.len(), 4);
    /// }
    /// # }
    /// # tokio::runtime::Runtime::new().unwrap().block_on(run());
    /// ```
    pub async fn bytes(self) -> crate::Result<Bytes> {
        let mut buf = BytesMut::new();

        let mut this = self;
        while let Some(bytes) = this.chunk().await? {
            buf.extend_from_slice(&bytes);
        }

        Ok(buf.freeze())
    }

    /// Stream a chunk of the field data.
    ///
    /// When the field data has been exhausted, this will return [`None`].
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
    /// while let Some(mut field) = multipart.next_field().await.unwrap() {
    ///     while let Some(chunk) = field.chunk().await.unwrap() {
    ///         println!("Chunk: {:?}", chunk);
    ///     }
    /// }
    /// # }
    /// # tokio::runtime::Runtime::new().unwrap().block_on(run());
    /// ```
    pub async fn chunk(&mut self) -> crate::Result<Option<Bytes>> {
        self.try_next().await
    }

    /// Try to deserialize the field data as JSON.
    ///
    /// # Optional
    ///
    /// This requires the optional `json` feature to be enabled.
    ///
    /// # Examples
    ///
    /// ```
    /// use multer::Multipart;
    /// use bytes::Bytes;
    /// use std::convert::Infallible;
    /// use futures_util::stream::once;
    /// use serde::Deserialize;
    ///
    /// // This `derive` requires the `serde` dependency.
    /// #[derive(Deserialize)]
    /// struct User {
    ///     name: String
    /// }
    ///
    /// # async fn run() {
    /// let data = "--X-BOUNDARY\r\nContent-Disposition: form-data; name=\"my_text_field\"\r\n\r\n{ \"name\": \"Alice\" }\r\n--X-BOUNDARY--\r\n";
    /// let stream = once(async move { Result::<Bytes, Infallible>::Ok(Bytes::from(data)) });
    /// let mut multipart = Multipart::new(stream, "X-BOUNDARY");
    ///
    /// while let Some(field) = multipart.next_field().await.unwrap() {
    ///     let user = field.json::<User>().await.unwrap();
    ///     println!("User Name: {}", user.name);
    /// }
    /// # }
    /// # tokio::runtime::Runtime::new().unwrap().block_on(run());
    /// ```
    ///
    /// # Errors
    ///
    /// This method fails if the field data is not in JSON format
    /// or it cannot be properly deserialized to target type `T`. For more
    /// details please see [`serde_json::from_slice`].
    #[cfg(feature = "json")]
    #[cfg_attr(nightly, doc(cfg(feature = "json")))]
    pub async fn json<T: DeserializeOwned>(self) -> crate::Result<T> {
        serde_json::from_slice(&self.bytes().await?).map_err(crate::Error::DecodeJson)
    }

    /// Get the full field data as text.
    ///
    /// This method decodes the field data with `BOM sniffing` and with
    /// malformed sequences replaced with the `REPLACEMENT CHARACTER`.
    /// Encoding is determined from the `charset` parameter of `Content-Type`
    /// header, and defaults to `utf-8` if not presented.
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
    ///     let content = field.text().await.unwrap();
    ///     assert_eq!(content, "abcd");
    /// }
    /// # }
    /// # tokio::runtime::Runtime::new().unwrap().block_on(run());
    /// ```
    pub async fn text(self) -> crate::Result<String> {
        self.text_with_charset("utf-8").await
    }

    /// Get the full field data as text given a specific encoding.
    ///
    /// This method decodes the field data with `BOM sniffing` and with
    /// malformed sequences replaced with the `REPLACEMENT CHARACTER`.
    /// You can provide a default encoding for decoding the raw message, while
    /// the `charset` parameter of `Content-Type` header is still prioritized.
    /// For more information about the possible encoding name, please go to
    /// [encoding_rs] docs.
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
    ///     let content = field.text_with_charset("utf-8").await.unwrap();
    ///     assert_eq!(content, "abcd");
    /// }
    /// # }
    /// # tokio::runtime::Runtime::new().unwrap().block_on(run());
    /// ```
    pub async fn text_with_charset(self, default_encoding: &str) -> crate::Result<String> {
        let encoding_name = self
            .content_type()
            .and_then(|mime| mime.get_param(mime::CHARSET))
            .map(|charset| charset.as_str())
            .unwrap_or(default_encoding);

        let encoding = Encoding::for_label(encoding_name.as_bytes()).unwrap_or(UTF_8);

        let bytes = self.bytes().await?;

        let (text, ..) = encoding.decode(&bytes);

        match text {
            Cow::Owned(s) => Ok(s),
            Cow::Borrowed(s) => Ok(String::from(s)),
        }
    }

    /// Get the index of this field in order they appeared in the stream.
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
    ///     let idx = field.index();
    ///     println!("Field index: {}", idx);
    /// }
    /// # }
    /// # tokio::runtime::Runtime::new().unwrap().block_on(run());
    /// ```
    pub fn index(&self) -> usize {
        self.meta.idx
    }
}

impl Stream for Field {
    type Item = Result<Bytes, crate::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.done {
            return Poll::Ready(None);
        }

        let mut mutex_guard = match self.state.lock() {
            Ok(lock) => lock,
            Err(err) => {
                return Poll::Ready(Some(Err(crate::Error::LockFailure(err.to_string().into()))));
            }
        };

        let state: &mut MultipartState = mutex_guard.deref_mut();

        let stream_buffer = &mut state.buffer;

        if let Err(err) = stream_buffer.poll_stream(cx) {
            return Poll::Ready(Some(Err(crate::Error::StreamReadFailed(err.into()))));
        }

        match stream_buffer.read_field_data(state.boundary.as_str(), state.curr_field_name.as_deref()) {
            Ok(Some((done, bytes))) => {
                state.curr_field_size_counter += bytes.len() as u64;

                if state.curr_field_size_counter > state.curr_field_size_limit {
                    return Poll::Ready(Some(Err(crate::Error::FieldSizeExceeded {
                        limit: state.curr_field_size_limit,
                        field_name: state.curr_field_name.clone(),
                    })));
                }

                drop(mutex_guard);

                if done {
                    self.done = true;
                    Poll::Ready(Some(Ok(bytes)))
                } else {
                    Poll::Ready(Some(Ok(bytes)))
                }
            }
            Ok(None) => Poll::Pending,
            Err(err) => Poll::Ready(Some(Err(err))),
        }
    }
}

impl Drop for Field {
    fn drop(&mut self) {
        let mut mutex_guard = match self.state.lock() {
            Ok(lock) => lock,
            Err(err) => {
                log::error!("{}", crate::Error::LockFailure(err.to_string().into()));
                return;
            }
        };

        let state: &mut MultipartState = mutex_guard.deref_mut();

        if self.done {
            state.stage = StreamingStage::ReadingBoundary;
        } else {
            state.stage = StreamingStage::CleaningPrevFieldData;
        }

        state.is_prev_field_consumed = true;

        if let Some(waker) = state.next_field_waker.take() {
            waker.wake();
        }
    }
}
