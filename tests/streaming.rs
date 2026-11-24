use std::convert::Infallible;
use std::error::Error as _;
use std::fmt;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use futures_util::{stream, FutureExt, Stream};
use multer::{Constraints, Error, Multipart, SizeLimit};
use tokio::sync::mpsc::{self, UnboundedSender};

#[derive(Debug)]
struct ReadFault(u32);

impl fmt::Display for ReadFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "read failed at {}", self.0)
    }
}
impl std::error::Error for ReadFault {}

type Item = Result<Bytes, ReadFault>;

// Each send makes one item ready. Keeping the sender open gates the next refill.
fn channel(polls: &AtomicUsize) -> (UnboundedSender<Item>, impl Stream<Item = Item> + '_) {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let stream = stream::poll_fn(move |cx| {
        polls.fetch_add(1, Ordering::Relaxed);
        rx.poll_recv(cx)
    });
    (tx, stream)
}

const FILE_HEAD: &[u8] = b"--B\r\nContent-Disposition: form-data; name=file; filename=a.bin\r\n\r\n";
const CLOSE: &[u8] = b"\r\n--B--";

#[tokio::test]
async fn ready_source_is_polled_only_on_demand() {
    let file: Vec<_> = (0..=255).cycle().take(65536).collect();
    let wire = [FILE_HEAD, &file, CLOSE].concat();
    for width in [4096, 16384, wire.len()] {
        let polls = AtomicUsize::new(0);
        let (tx, source) = channel(&polls);
        for piece in wire.chunks(width) {
            tx.send(Ok(Bytes::copy_from_slice(piece))).unwrap();
        }
        // A separately supplied epilogue is outside the form.
        tx.send(Err(ReadFault(42))).unwrap();
        let mut m = Multipart::new(source, "B");
        let mut field = m.next_field().await.unwrap().unwrap();
        assert_eq!(field.file_name(), Some("a.bin"));
        assert_eq!(polls.load(Ordering::Relaxed), 1);
        tokio::task::yield_now().await;
        let first = field.chunk().await.unwrap().unwrap();
        assert!(!first.is_empty());
        assert!(first.len() <= 16 * 1024);
        assert_eq!(polls.load(Ordering::Relaxed), 1);
        assert_eq!(first.as_ref(), &file[..first.len()]);
        assert_eq!(field.bytes().await.unwrap().as_ref(), &file[first.len()..]);
        assert!(m.next_field().await.unwrap().is_none());
        assert!(m.next_field().await.unwrap().is_none());
        assert_eq!(polls.load(Ordering::Relaxed), wire.chunks(width).count());
    }
}

#[cfg(feature = "tokio-io")]
#[tokio::test]
async fn reader_returns_buffered_field_and_body_after_one_bounded_read() {
    use std::pin::Pin;
    use std::task::{Context, Poll};
    use tokio::io::{AsyncRead, ReadBuf};

    struct CountReads<'a> {
        data: &'a [u8],
        polls: &'a AtomicUsize,
    }
    impl AsyncRead for CountReads<'_> {
        fn poll_read(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &mut ReadBuf<'_>,
        ) -> Poll<std::io::Result<()>> {
            self.polls.fetch_add(1, Ordering::Relaxed);
            assert!((1..=16 * 1024).contains(&buf.remaining()));
            Pin::new(&mut self.data).poll_read(cx, buf)
        }
    }

    let file: Vec<_> = (0..=255).cycle().take(65536).collect();
    let wire = [FILE_HEAD, &file, CLOSE].concat();
    let polls = AtomicUsize::new(0);
    let reader = CountReads {
        data: &wire,
        polls: &polls,
    };
    let mut m = Multipart::with_reader(reader, "B");
    let mut field = m.next_field().await.unwrap().unwrap();
    assert_eq!((field.name(), field.file_name()), (Some("file"), Some("a.bin")));
    assert_eq!(polls.load(Ordering::Relaxed), 1);
    tokio::task::yield_now().await;
    let prefix = field.chunk().await.unwrap().unwrap();
    assert_eq!(polls.load(Ordering::Relaxed), 1);
    assert!(!prefix.is_empty());
    assert_eq!(prefix.as_ref(), &file[..prefix.len()]);
    assert_eq!(field.bytes().await.unwrap().as_ref(), &file[prefix.len()..]);
    assert!(m.next_field().await.unwrap().is_none());
}

#[tokio::test]
async fn delimiters_at_every_split() {
    let prefix = b"preamble --aba-- in text\n--aba after LF\r--aba after CR\r\n";
    let header = b"--aba\t \r\nContent-Disposition: form-data; name=file\r\n\r\n";
    let data = b"binary\xff\r\n--abX\r\r\n--a\r\n--z";
    let tail = b"\r\n--aba \t\r\nContent-Disposition: form-data; name=last\r\n\r\nyes\r\n--aba--";
    let wire = [&prefix[..], header, data, tail].concat();
    for split in 0..=wire.len() {
        for skip in 0..3 {
            let polls = AtomicUsize::new(0);
            let (tx, source) = channel(&polls);
            tx.send(Ok(Bytes::copy_from_slice(&wire[..split]))).unwrap();
            let mut m = Multipart::new(source, "aba");
            // If this split has supplied the headers, hold the live field over the pause.
            let first = m.next_field().now_or_never();
            tx.send(Ok(Bytes::copy_from_slice(&wire[split..]))).unwrap();
            let mut field = match first {
                Some(result) => result,
                None => m.next_field().await,
            }
            .unwrap()
            .unwrap();
            assert_eq!((field.index(), field.name()), (0, Some("file")));
            assert_eq!(field.headers()["content-disposition"], "form-data; name=file");
            match skip {
                0 => assert_eq!(field.bytes().await.unwrap().as_ref(), data),
                1 => {
                    let prefix = field.chunk().await.unwrap().unwrap();
                    assert!(data.starts_with(&prefix));
                    drop(field);
                }
                _ => drop(field),
            }
            let next = m.next_field().await.unwrap().unwrap();
            assert_eq!((next.index(), next.name()), (1, Some("last")));
            assert_eq!(next.text().await.unwrap(), "yes");
            assert!(m.next_field().await.unwrap().is_none());
        }
    }
    let pieces = stream::iter(wire.chunks(1).map(|b| Ok::<_, Infallible>(Bytes::copy_from_slice(b))));
    let mut m = Multipart::new(pieces, "aba");
    assert_eq!(
        m.next_field().await.unwrap().unwrap().bytes().await.unwrap().as_ref(),
        data
    );
    assert_eq!(m.next_field().await.unwrap().unwrap().text().await.unwrap(), "yes");
    assert!(m.next_field().await.unwrap().is_none());
}

#[tokio::test]
async fn opening_boundary_requires_a_line_start() {
    for prefix in ["", "preamble\r\n", "text", "text\r", "text\n"] {
        let wire = format!("{prefix}--B--");
        for width in [1, 2, wire.len()] {
            let source = stream::iter(
                wire.as_bytes()
                    .chunks(width)
                    .map(|b| Ok::<_, Infallible>(Bytes::copy_from_slice(b))),
            );
            let mut m = Multipart::new(source, "B");
            if prefix.is_empty() || prefix.ends_with("\r\n") {
                assert!(m.next_field().await.unwrap().is_none());
            } else {
                assert!(matches!(m.next_field().await, Err(Error::IncompleteStream)));
                assert!(matches!(m.next_field().await, Err(Error::MultipartFailed)));
            }
        }
    }
}

#[tokio::test]
async fn pending_reads_resume_across_cancellation() {
    let polls = AtomicUsize::new(0);
    let (tx, source) = channel(&polls);
    let constraints = Constraints::new()
        .size_limit(SizeLimit::new().per_field(9))
        .max_fields(2);
    let mut m = Multipart::with_constraints(source, "B", constraints);
    tx.send(Ok(Bytes::from_static(b"preamble\r\n-"))).unwrap();
    assert!(m.next_field().now_or_never().is_none());
    tx.send(Ok(Bytes::from_static(b"-B\r\nContent-Disposition: form-data; na")))
        .unwrap();
    assert!(m.next_field().now_or_never().is_none());
    tx.send(Ok(Bytes::from_static(b"me=file\r\n\r\na\r\n-"))).unwrap();
    let mut field = m.next_field().await.unwrap().unwrap();
    assert_eq!((field.index(), field.name()), (0, Some("file")));
    assert_eq!(field.chunk().await.unwrap().unwrap(), "a");
    assert!(field.chunk().now_or_never().is_none());
    tx.send(Ok(Bytes::from_static(b"Xabc"))).unwrap();
    assert_eq!(field.chunk().await.unwrap().unwrap(), "\r\n-Xabc");
    drop(field);
    assert!(m.next_field().now_or_never().is_none());
    tx.send(Ok(Bytes::from_static(b"!\r\n--B\r\n\r\nlast\r\n--B--")))
        .unwrap();
    let field = m.next_field().await.unwrap().unwrap();
    assert_eq!((field.index(), field.name()), (1, None));
    assert_eq!(field.text().await.unwrap(), "last");
    assert!(m.next_field().await.unwrap().is_none());
}

#[tokio::test]
async fn empty_refills_yield_and_wake_the_task() {
    let polls = AtomicUsize::new(0);
    let (tx, source) = channel(&polls);
    for _ in 0..96 {
        tx.send(Ok(Bytes::new())).unwrap();
    }
    let mut m = Multipart::new(source, "B");
    assert!(m.next_field().now_or_never().is_none());
    assert!((1..96).contains(&polls.load(Ordering::Relaxed)));
    // Wake the source from another task. The parser also has to repoll itself
    // when the finite run of ready empty items exceeds its local work budget.
    let release = tokio::spawn(async move {
        tokio::task::yield_now().await;
        tx.send(Ok(Bytes::from_static(b"--B\r\n\r\ntext\r\n--B--"))).unwrap();
    });
    let field = tokio::time::timeout(Duration::from_secs(2), m.next_field())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(field.text().await.unwrap(), "text");
    assert!(m.next_field().await.unwrap().is_none());
    release.await.unwrap();
}

#[tokio::test]
async fn field_and_parent_share_the_original_source_error() {
    let polls = AtomicUsize::new(0);
    let (tx, source) = channel(&polls);
    tx.send(Ok(Bytes::from_static(FILE_HEAD))).unwrap();
    tx.send(Err(ReadFault(17))).unwrap();
    let mut m = Multipart::new(source, "B");
    let mut field = m.next_field().await.unwrap().unwrap();
    let field_error = field.chunk().await.unwrap_err();
    assert!(field.chunk().await.unwrap().is_none());
    drop(field);
    let parent_error = m.next_field().await.unwrap_err();
    let from_field = field_error.source().unwrap().downcast_ref::<ReadFault>().unwrap();
    let from_parent = parent_error.source().unwrap().downcast_ref::<ReadFault>().unwrap();
    assert!(std::ptr::eq(from_field, from_parent));
    let cause = match parent_error {
        Error::StreamReadFailed(cause) => cause,
        _ => panic!("expected source error"),
    };
    assert!(std::ptr::eq(cause.downcast_ref().unwrap(), from_field));
    let cause = Arc::try_unwrap(cause).unwrap_err();
    drop(field_error);
    assert_eq!(Arc::try_unwrap(cause).unwrap().downcast::<ReadFault>().unwrap().0, 17);
    assert!(matches!(m.next_field().await, Err(Error::MultipartFailed)));
    assert_eq!(polls.load(Ordering::Relaxed), 2);
}

#[tokio::test]
async fn terminal_field_failures_survive_a_drop() {
    for complete in [true, false] {
        let tail = if complete { &b"abcd\r\n--B--"[..] } else { &b"ab"[..] };
        let wire = [FILE_HEAD, tail].concat();
        let polls = AtomicUsize::new(0);
        let (tx, source) = channel(&polls);
        tx.send(Ok(Bytes::from(wire))).unwrap();
        drop(tx);
        let mut m = Multipart::with_constraints(
            source,
            "B",
            Constraints::new().size_limit(SizeLimit::new().per_field(3)),
        );
        let field = m.next_field().await.unwrap().unwrap();
        let error = field.bytes().await.unwrap_err();
        let stopped = polls.load(Ordering::Relaxed);
        for err in [error, m.next_field().await.unwrap_err()] {
            if complete {
                assert!(
                    matches!(err, Error::FieldSizeExceeded { limit: 3, field_name } if field_name.as_deref() == Some("file"))
                );
            } else {
                assert!(
                    matches!(err, Error::IncompleteFieldData { field_name } if field_name.as_deref() == Some("file"))
                );
            }
        }
        assert!(matches!(m.next_field().await, Err(Error::MultipartFailed)));
        assert_eq!(polls.load(Ordering::Relaxed), stopped);
    }
}

#[cfg(feature = "json")]
#[tokio::test]
async fn json_conversion_failure_leaves_the_next_part_available() {
    let wire = b"--B\r\n\r\nx\r\n--B\r\n\r\n42\r\n--B--".as_slice();
    let source = stream::iter([Ok::<_, Infallible>(Bytes::from_static(wire))]);
    let mut m = Multipart::new(source, "B");
    assert!(matches!(
        m.next_field().await.unwrap().unwrap().json::<u32>().await,
        Err(Error::DecodeJson(_))
    ));
    assert_eq!(m.next_field().await.unwrap().unwrap().json::<u32>().await.unwrap(), 42);
    assert!(m.next_field().await.unwrap().is_none());
}

#[tokio::test]
async fn count_is_checked_at_admission() {
    for limit in [0, 2] {
        let polls = AtomicUsize::new(0);
        let (tx, source) = channel(&polls);
        let constraints = Constraints::new().max_fields(limit).size_limit(SizeLimit::new());
        let mut m = Multipart::with_constraints(source, "B", constraints);
        if limit == 2 {
            tx.send(Ok(Bytes::from_static(
                b"--B\r\n\r\n\r\n--B\r\nContent-Disposition: form-data; name=x\r\n\r\ntwo\r\n--B\r\n",
            )))
            .unwrap();
            let first = m.next_field().await.unwrap().unwrap();
            assert_eq!((first.index(), first.name()), (0, None));
            assert!(first.bytes().await.unwrap().is_empty());
            let second = m.next_field().await.unwrap().unwrap();
            assert_eq!((second.index(), second.name()), (1, Some("x")));
            drop(second);
        } else {
            tx.send(Ok(Bytes::from_static(b"--B\r\n"))).unwrap();
        }
        assert!(matches!(m.next_field().now_or_never(), Some(Err(Error::TooManyFields { limit: n })) if n == limit));
        assert!(matches!(m.next_field().await, Err(Error::MultipartFailed)));
        assert_eq!(polls.load(Ordering::Relaxed), 1);
    }

    let source = stream::iter([Ok::<_, Infallible>(Bytes::from_static(b"--B--"))]);
    let mut m = Multipart::with_constraints(source, "B", Constraints::new().max_fields(0));
    assert!(m.next_field().await.unwrap().is_none());

    let polls = AtomicUsize::new(0);
    let (tx, source) = channel(&polls);
    let constraints = Constraints::new()
        .allowed_fields(vec!["x"])
        .size_limit(SizeLimit::new())
        .max_fields(2);
    let mut m = Multipart::with_constraints(source, "B", constraints);
    tx.send(Ok(Bytes::from_static(b"--B\r\nContent-Disposition: form-data; name=x\r\n\r\n\r\n--B\r\nContent-Disposition: form-data; name=x\r\n\r\nyes\r\n--B--"))).unwrap();
    for i in 0..2 {
        let field = m.next_field().await.unwrap().unwrap();
        assert_eq!((field.index(), field.name()), (i, Some("x")));
    }
    assert!(m.next_field().now_or_never().unwrap().unwrap().is_none());
    assert_eq!(polls.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn an_entire_fetched_item_counts_once() {
    let mut wire = [b"Preamble: ordinary\r\n".as_slice(), FILE_HEAD, b"abc", CLOSE].concat();
    wire.resize(20000, b' ');
    let size = wire.len() as u64;
    for limit in [size - 1, size] {
        let polls = AtomicUsize::new(0);
        let (tx, source) = channel(&polls);
        tx.send(Ok(Bytes::copy_from_slice(&wire))).unwrap();
        let mut m = Multipart::with_constraints(
            source,
            "B",
            Constraints::new().size_limit(SizeLimit::new().whole_stream(limit)),
        );
        if limit == size {
            assert!(m.next_field().await.unwrap().is_some());
            assert!(m.next_field().await.unwrap().is_none());
        } else {
            assert!(matches!(m.next_field().await, Err(Error::StreamSizeExceeded { limit: n }) if n == limit));
            assert!(matches!(m.next_field().await, Err(Error::MultipartFailed)));
        }
        assert_eq!(polls.load(Ordering::Relaxed), 1);
    }
}

#[tokio::test]
async fn field_and_parent_report_a_later_whole_stream_limit() {
    let polls = AtomicUsize::new(0);
    let (tx, source) = channel(&polls);
    let limit = FILE_HEAD.len() as u64;
    tx.send(Ok(Bytes::from_static(FILE_HEAD))).unwrap();
    tx.send(Ok(Bytes::from_static(b"abc\r\n--B--"))).unwrap();
    let mut m = Multipart::with_constraints(
        source,
        "B",
        Constraints::new().size_limit(SizeLimit::new().whole_stream(limit)),
    );
    let field = m.next_field().await.unwrap().unwrap();
    for err in [field.bytes().await.unwrap_err(), m.next_field().await.unwrap_err()] {
        assert!(matches!(err, Error::StreamSizeExceeded { limit: n } if n == limit));
    }
    assert!(matches!(m.next_field().await, Err(Error::MultipartFailed)));
    assert_eq!(polls.load(Ordering::Relaxed), 2);
}

#[tokio::test]
async fn header_limits_at_section_boundaries() {
    for header in [&b"X: v\r\n\r\n"[..], b"\r\n"] {
        // The following part remains distinct after a headerless payload.
        let next = b"\r\n--B\r\nContent-Disposition: form-data; name=next\r\n\r\nok\r\n--B--";
        let wire = [&b"--B\r\n"[..], header, b"body", next].concat();
        let len = header.len() as u64;
        for width in [wire.len(), 1] {
            for limit in [len - 1, len, len + 1, 1 << 32] {
                let source = stream::iter(
                    wire.chunks(width)
                        .map(|v| Ok::<_, Infallible>(Bytes::copy_from_slice(v))),
                );
                let mut m = Multipart::with_constraints(
                    source,
                    "B",
                    Constraints::new().size_limit(SizeLimit::new().per_header(limit)),
                );
                if limit < len {
                    assert!(
                        matches!(m.next_field().await, Err(Error::HeaderSizeExceeded { limit: n, header: None }) if n == limit)
                    );
                } else {
                    let first = m.next_field().await.unwrap().unwrap();
                    assert_eq!((first.index(), first.name()), (0, None));
                    assert_eq!(first.headers().is_empty(), header == b"\r\n");
                    assert_eq!(first.text().await.unwrap(), "body");
                    if limit >= 1 << 32 {
                        let second = m.next_field().await.unwrap().unwrap();
                        assert_eq!((second.index(), second.name()), (1, Some("next")));
                        assert_eq!(second.text().await.unwrap(), "ok");
                        assert!(m.next_field().await.unwrap().is_none());
                    }
                }
            }
        }
    }

    let polls = AtomicUsize::new(0);
    let (tx, source) = channel(&polls);
    tx.send(Ok(Bytes::from_static(b"--B\r\nX: v"))).unwrap();
    let mut m = Multipart::with_constraints(
        source,
        "B",
        Constraints::new().size_limit(SizeLimit::new().per_header(3)),
    );
    assert!(matches!(
        m.next_field().now_or_never(),
        Some(Err(Error::HeaderSizeExceeded { limit: 3, header: None }))
    ));
    assert_eq!(polls.load(Ordering::Relaxed), 1);

    for count in [32, 33] {
        let mut wire = "--B\r\n".to_owned();
        for i in 0..count {
            wire.push_str(&format!("X-{i}: v\r\n"));
        }
        wire.push_str("\r\n\r\n--B--");
        let source = stream::iter([Ok::<_, Infallible>(Bytes::from(wire))]);
        let mut m = Multipart::new(source, "B");
        if count == 32 {
            assert_eq!(m.next_field().await.unwrap().unwrap().headers().len(), count);
            assert!(m.next_field().await.unwrap().is_none());
        } else {
            assert!(matches!(
                m.next_field().await,
                Err(Error::ReadHeaderFailed(httparse::Error::TooManyHeaders))
            ));
            assert!(matches!(m.next_field().await, Err(Error::MultipartFailed)));
        }
    }
    assert!(
        matches!(SizeLimit::new().try_for_header("a b", 5), Err(Error::DecodeHeaderName { name, .. }) if name == "a b")
    );
}

#[tokio::test]
async fn keep_or_skip_a_partially_read_body_under_its_limit() {
    for (skip, limit) in [(false, 6), (true, 6), (true, 5)] {
        let polls = AtomicUsize::new(0);
        let (tx, source) = channel(&polls);
        let constraints = Constraints::new().size_limit(SizeLimit::new().per_field(0).for_field("file", limit));
        let mut m = Multipart::with_constraints(source, "B", constraints);
        tx.send(Ok(Bytes::from([FILE_HEAD, b"abc"].concat()))).unwrap();
        let mut field = m.next_field().await.unwrap().unwrap();
        let first = field.chunk().await.unwrap().unwrap();
        assert_eq!(first, "abc");
        tx.send(Ok(Bytes::from_static(b"def\r\n--B--"))).unwrap();
        if skip {
            drop(field);
        } else {
            assert_eq!(field.bytes().await.unwrap(), "def");
        }
        if limit == 6 {
            assert!(m.next_field().await.unwrap().is_none());
        } else {
            assert!(
                matches!(m.next_field().await, Err(Error::FieldSizeExceeded { limit: 5, field_name }) if field_name.as_deref() == Some("file"))
            );
            assert!(matches!(m.next_field().await, Err(Error::MultipartFailed)));
        }
        drop(m);
        assert_eq!(first, "abc");
    }
}
