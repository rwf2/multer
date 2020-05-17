use crate::constants;
use bytes::{Bytes, BytesMut};
use futures::stream::Stream;
use std::pin::Pin;
use std::task::{Context, Poll};

pub(crate) struct StreamBuffer {
    pub(crate) eof: bool,
    pub(crate) buf: BytesMut,
    pub(crate) stream: Pin<Box<dyn Stream<Item = Result<Bytes, crate::Error>> + Send>>,
    pub(crate) whole_stream_size_limit: usize,
    pub(crate) stream_size_counter: usize,
}

impl StreamBuffer {
    pub fn new<S>(stream: S, whole_stream_size_limit: usize) -> Self
    where
        S: Stream<Item = Result<Bytes, crate::Error>> + Send + 'static,
    {
        StreamBuffer {
            eof: false,
            buf: BytesMut::new(),
            stream: Box::pin(stream),
            whole_stream_size_limit,
            stream_size_counter: 0,
        }
    }

    pub fn poll_stream(&mut self, cx: &mut Context) -> Result<(), crate::Error> {
        if self.eof {
            return Ok(());
        }

        loop {
            match self.stream.as_mut().poll_next(cx) {
                Poll::Ready(Some(Ok(data))) => {
                    self.stream_size_counter += data.len();

                    if self.stream_size_counter > self.whole_stream_size_limit {
                        return Err(crate::Error::StreamSizeExceeded {
                            limit: self.whole_stream_size_limit,
                        });
                    }

                    self.buf.extend_from_slice(&data)
                }
                Poll::Ready(Some(Err(err))) => return Err(err),
                Poll::Ready(None) => {
                    self.eof = true;
                    return Ok(());
                }
                Poll::Pending => return Ok(()),
            }
        }
    }

    pub fn read_exact(&mut self, size: usize) -> Option<Bytes> {
        if size <= self.buf.len() {
            Some(self.buf.split_to(size).freeze())
        } else {
            None
        }
    }

    pub fn read_until(&mut self, pattern: &[u8]) -> Option<Bytes> {
        twoway::find_bytes(&self.buf, pattern).map(|idx| self.buf.split_to(idx + pattern.len()).freeze())
    }

    pub fn read_field_data(
        &mut self,
        boundary: &str,
        field_name: Option<&str>,
    ) -> crate::Result<Option<(bool, Bytes)>> {
        if self.buf.is_empty() {
            return if self.eof {
                Err(crate::Error::IncompleteFieldData {
                    field_name: field_name.map(|s| s.to_owned()),
                })
            } else {
                Ok(None)
            };
        }

        let boundary_deriv = format!("{}{}{}", constants::CRLF, constants::BOUNDARY_EXT, boundary);
        let b_len = boundary_deriv.len();

        match twoway::find_bytes(&self.buf, boundary_deriv.as_bytes()) {
            Some(idx) => {
                let bytes = self.buf.split_to(idx).freeze();

                // discard \r\n.
                drop(self.buf.split_to(2).freeze());

                Ok(Some((true, bytes)))
            }
            None => {
                let buf_len = self.buf.len();
                let rem_boundary_part_max_len = b_len - 1;
                let rem_boundary_part_idx;

                if buf_len >= rem_boundary_part_max_len {
                    rem_boundary_part_idx = buf_len - rem_boundary_part_max_len
                } else {
                    rem_boundary_part_idx = 0
                }

                match twoway::rfind_bytes(&self.buf[rem_boundary_part_idx..], constants::CR.as_bytes()) {
                    Some(rel_idx) => {
                        let idx = rel_idx + rem_boundary_part_idx;

                        match twoway::find_bytes(boundary_deriv.as_bytes(), &self.buf[idx..]) {
                            Some(_) => {
                                let bytes = self.buf.split_to(idx).freeze();

                                if self.eof {
                                    Err(crate::Error::IncompleteFieldData {
                                        field_name: field_name.map(|s| s.to_owned()),
                                    })
                                } else {
                                    if bytes.is_empty() {
                                        Ok(None)
                                    } else {
                                        Ok(Some((false, bytes)))
                                    }
                                }
                            }
                            None => {
                                if self.eof {
                                    Err(crate::Error::IncompleteFieldData {
                                        field_name: field_name.map(|s| s.to_owned()),
                                    })
                                } else {
                                    Ok(Some((false, self.read_full_buf())))
                                }
                            }
                        }
                    }
                    None => {
                        if self.eof {
                            Err(crate::Error::IncompleteFieldData {
                                field_name: field_name.map(|s| s.to_owned()),
                            })
                        } else {
                            Ok(Some((false, self.read_full_buf())))
                        }
                    }
                }
            }
        }
    }

    pub fn read_full_buf(&mut self) -> Bytes {
        self.buf.split_to(self.buf.len()).freeze()
    }
}
