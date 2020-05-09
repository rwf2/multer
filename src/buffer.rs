use crate::constants;
use bytes::{Bytes, BytesMut};
use futures::stream::Stream;
use std::pin::Pin;
use std::task::{Context, Poll};

pub(crate) struct StreamBuffer<S> {
    pub(crate) eof: bool,
    pub(crate) buf: BytesMut,
    pub(crate) stream: S,
}

impl<S: Stream<Item = Result<Bytes, crate::Error>> + Send + Sync + Unpin + 'static> StreamBuffer<S> {
    pub fn new(stream: S) -> Self {
        StreamBuffer {
            eof: false,
            buf: BytesMut::new(),
            stream,
        }
    }

    pub fn poll_stream(&mut self, cx: &mut Context) -> Result<(), crate::Error> {
        if self.eof {
            return Ok(());
        }

        loop {
            match Pin::new(&mut self.stream).poll_next(cx) {
                Poll::Ready(Some(Ok(data))) => {
                    // println!("poll chunk size: {}", data.len());
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

    pub fn read_field_data(&mut self, boundary: &str) -> crate::Result<Option<(bool, Bytes)>> {
        if self.buf.is_empty() {
            return if self.eof {
                Err(crate::Error::new("Incomplete field data"))
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

                println!("boundary found");

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

                        println!("CR found at the end: {}", rel_idx);

                        match twoway::find_bytes(boundary_deriv.as_bytes(), &self.buf[idx..]) {
                            Some(_) => {
                                println!("End CR matched with boundary part");
                                let bytes = self.buf.split_to(idx).freeze();

                                if self.eof {
                                    Err(crate::Error::new("Incomplete field data"))
                                } else {
                                    if bytes.is_empty() {
                                        Ok(None)
                                    } else {
                                        Ok(Some((false, bytes)))
                                    }
                                }
                            }
                            None => {
                                println!("End CR didn't match with boundary part");
                                if self.eof {
                                    Err(crate::Error::new("Incomplete field data"))
                                } else {
                                    Ok(Some((false, self.read_full_buf())))
                                }
                            }
                        }
                    }
                    None => {
                        println!("CR not found at the end");
                        if self.eof {
                            Err(crate::Error::new("Incomplete field data"))
                        } else {
                            Ok(Some((false, self.read_full_buf())))
                        }
                    }
                }
            }
        }
    }

    // pub fn read_field_data(&mut self, boundary: &str) -> crate::Result<Option<(bool, Bytes)>> {
    //     if self.buf.is_empty() {
    //         return if self.eof {
    //             Err(crate::Error::new("Incomplete field data"))
    //         } else {
    //             Ok(None)
    //         };
    //     }
    //
    //     match twoway::find_bytes(&self.buf, constants::CR.as_bytes()) {
    //         Some(idx) => {
    //             let boundary_deriv = format!("{}{}{}", constants::CRLF, constants::BOUNDARY_EXT, boundary);
    //             let b_len = boundary_deriv.len();
    //
    //             if self.buf.len() >= (idx + b_len) {
    //                 if &self.buf[idx..(idx + b_len)] == boundary_deriv.as_bytes() {
    //                     let bytes = self.buf.split_to(idx).freeze();
    //
    //                     // discard \r\n.
    //                     drop(self.buf.split_to(2).freeze());
    //
    //                     Ok(Some((true, bytes)))
    //                 } else {
    //                     // @todo: this path is being called multiple times because binary files are likely to have many \r character.
    //                     let bytes = self.buf.split_to(idx + 1).freeze();
    //                     println!("Found, but not matched: {}", bytes.len());
    //                     Ok(Some((false, bytes)))
    //                 }
    //             } else {
    //                 if self.eof {
    //                     Err(crate::Error::new("Incomplete field data"))
    //                 } else {
    //                     Ok(None)
    //                 }
    //             }
    //         }
    //         None => {
    //             println!("No found: {}", self.buf.len());
    //             if self.eof {
    //                 Err(crate::Error::new("Incomplete field data"))
    //             } else {
    //                 Ok(Some((false, self.read_full_buf())))
    //             }
    //         }
    //     }
    // }

    pub fn read_full_buf(&mut self) -> Bytes {
        self.buf.split_to(self.buf.len()).freeze()
    }

    // pub fn read_max(&mut self, size: u64) -> Result<Option<Bytes>, MultipartError> {
    //     if !self.buf.is_empty() {
    //         let size = std::cmp::min(self.buf.len() as u64, size) as usize;
    //         Ok(Some(self.buf.split_to(size).freeze()))
    //     } else if self.eof {
    //         Err(MultipartError::Incomplete)
    //     } else {
    //         Ok(None)
    //     }
    // }
    //

    //
    // /// Read bytes until new line delimiter
    // pub fn readline(&mut self) -> Result<Option<Bytes>, MultipartError> {
    //     self.read_until(b"\n")
    // }
    //
    // /// Read bytes until new line delimiter or eof
    // pub fn readline_or_eof(&mut self) -> Result<Option<Bytes>, MultipartError> {
    //     match self.readline() {
    //         Err(MultipartError::Incomplete) if self.eof => Ok(Some(self.buf.split().freeze())),
    //         line => line,
    //     }
    // }
    //
    // /// Put unprocessed data back to the buffer
    // pub fn unprocessed(&mut self, data: Bytes) {
    //     let buf = BytesMut::from(data.as_ref());
    //     let buf = std::mem::replace(&mut self.buf, buf);
    //     self.buf.extend_from_slice(&buf);
    // }
}
