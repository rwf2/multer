use std::convert::Infallible;

use bytes::Bytes;
use futures_util::{stream, Stream, StreamExt};
use multer::{Constraints, Field, Multipart};

fn pieces(wire: &[u8], width: usize) -> impl Stream<Item = Result<Bytes, Infallible>> + '_ {
    stream::iter(wire.chunks(width).map(|chunk| Ok(Bytes::copy_from_slice(chunk))))
}

fn public_traits<T: Send + Sync + Unpin>(_: &T) {}

async fn check_capture(mut multipart: Multipart<'_>) {
    public_traits(&multipart);
    let binary: Vec<u8> = (0..=255).collect();
    let fields = [
        ("tag", None, &b"first"[..]),
        ("tag", None, &b"second"[..]),
        ("empty", None, &b""[..]),
        ("upload", Some("café-雪.bin"), &binary[..]),
    ];
    let mut held = Vec::new();
    for (index, &(name, filename, body)) in fields.iter().enumerate() {
        let (idx, mut field) = multipart.next_field_with_idx().await.unwrap().unwrap();
        public_traits(&field);
        assert_eq!((idx, field.index()), (index, index));
        assert_eq!(field.name(), Some(name));
        assert_eq!(field.file_name(), filename);
        let mut disposition = format!("form-data; name=\"{name}\"");
        if let Some(filename) = filename {
            disposition.push_str(&format!("; filename=\"{filename}\""));
            assert_eq!(field.content_type(), Some(&mime::APPLICATION_OCTET_STREAM));
            assert_eq!(field.headers()["content-type"].as_bytes(), b"application/octet-stream");
            assert_eq!(field.headers().len(), 2);
        } else {
            assert!(field.content_type().is_none());
            assert_eq!(field.headers().len(), 1);
        }
        assert_eq!(
            field.headers()["Content-Disposition"].as_bytes(),
            disposition.as_bytes()
        );
        if index == 0 {
            while let Some(chunk) = field.next().await {
                held.push(chunk.unwrap());
            }
            assert!(field.chunk().await.unwrap().is_none());
            assert!(matches!(multipart.next_field().await, Err(multer::Error::LockFailure)));
        } else {
            assert_eq!(field.bytes().await.unwrap().as_ref(), body);
        }
    }
    for _ in 0..2 {
        assert!(multipart.next_field().await.unwrap().is_none());
    }
    drop(multipart);
    assert_eq!(held.concat(), b"first");
}

#[tokio::test]
async fn form_data_capture() {
    // Node FormData serialized these four entries in order.
    let wire = include_bytes!("fixtures/form-data.bin");
    let boundary = multer::parse_boundary("multipart/form-data; boundary=----formdata-undici-070677391892").unwrap();
    for width in [1, 17, wire.len()] {
        check_capture(Multipart::new(pieces(wire, width), &boundary)).await;
    }
    #[cfg(feature = "tokio-io")]
    {
        check_capture(Multipart::with_reader(wire.as_slice(), &boundary)).await;
        check_capture(Multipart::with_reader_with_constraints(
            wire.as_slice(),
            &boundary,
            Constraints::new(),
        ))
        .await;
    }
}

#[tokio::test]
async fn read_or_skip_middle_field() {
    for middle in ["", "abc", "a longer field"] {
        let mut wire = Vec::new();
        for (name, body) in [("tag", ""), ("tag", middle), ("", "last")] {
            wire.extend_from_slice(
                format!("--B\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{body}\r\n").as_bytes(),
            );
        }
        wire.extend_from_slice(b"--B--");
        for width in [1, 9, wire.len()] {
            for consume in 0..3 {
                let mut m = Multipart::with_constraints(pieces(&wire, width), "B", Constraints::new());
                let first = m.next_field().await.unwrap().unwrap();
                assert_eq!((first.name(), first.index()), (Some("tag"), 0));
                assert!(first.bytes().await.unwrap().is_empty());
                let mut field = m.next_field().await.unwrap().unwrap();
                assert_eq!((field.name(), field.index()), (Some("tag"), 1));
                match consume {
                    0 => drop(field),
                    1 => {
                        let prefix = field.chunk().await.unwrap().unwrap();
                        assert!(middle.as_bytes().starts_with(&prefix));
                        drop(field);
                    }
                    _ => assert_eq!(field.bytes().await.unwrap().as_ref(), middle.as_bytes()),
                }
                let last = m.next_field().await.unwrap().unwrap();
                assert_eq!((last.name(), last.index()), (Some(""), 2));
                assert_eq!(last.text().await.unwrap(), "last");
                assert!(m.next_field().await.unwrap().is_none());
            }
        }
    }
}

async fn detached_field(headers: &[u8], body: &[u8]) -> Field<'static> {
    let mut wire = b"--B\r\n".to_vec();
    wire.extend_from_slice(headers);
    wire.extend_from_slice(b"\r\n\r\n");
    wire.extend_from_slice(body);
    wire.extend_from_slice(b"\r\n--B--");
    let mut m = Multipart::new(stream::iter([Ok::<_, Infallible>(wire)]), "B");
    m.next_field().await.unwrap().unwrap()
}

#[tokio::test]
async fn field_metadata_and_text_decoding() {
    let headers = b"Content-Disposition: form-data; name=label; filename=\"\"\r\nContent-Type: plain\r\nX-User: first\r\nx-user: \x80\xff";
    let field = detached_field(headers, b"value").await;
    assert_eq!(field.name(), Some("label"));
    assert_eq!(field.file_name(), Some(""));
    assert!(field.content_type().is_none());
    assert_eq!(field.headers()["content-type"].as_bytes(), b"plain");
    assert_eq!(field.headers().get_all("x-user").iter().count(), 1);
    assert_eq!(field.headers()["x-user"].as_bytes(), &[0x80, 0xff]);
    assert_eq!(field.text().await.unwrap(), "value");

    let text = b"Content-Disposition: form-data; name=text";
    let bom_text = b"\xef\xbb\xbf42";
    assert_eq!(
        detached_field(text, bom_text).await.bytes().await.unwrap().as_ref(),
        bom_text
    );
    assert_eq!(detached_field(text, bom_text).await.text().await.unwrap(), "42");
    assert_eq!(
        detached_field(text, b"a\xffz").await.text().await.unwrap(),
        "a\u{fffd}z"
    );
    assert_eq!(
        detached_field(text, &[0x80])
            .await
            .text_with_charset("windows-1252")
            .await
            .unwrap(),
        "€"
    );
    let charset = b"Content-Type: text/plain; charset=windows-1252";
    assert_eq!(
        detached_field(charset, &[0x80])
            .await
            .text_with_charset("utf-8")
            .await
            .unwrap(),
        "€"
    );
    assert_eq!(
        detached_field(charset, b"\xff\xfe4\x002\x00")
            .await
            .text()
            .await
            .unwrap(),
        "42"
    );
}
