use bytes::Bytes;
use futures::stream;
use multer::Multipart;

#[tokio::test]
async fn test_multipart_basic() {
    let data = "--X-BOUNDARY\r\nContent-Disposition: form-data; name=\"My Field\"\r\n\r\nabcd\r\n--X-BOUNDARY\r\nContent-Disposition: form-data; name=\"File Field\"; filename=\"a-text-file.txt\"\r\nContent-Type: text/plain\r\n\r\nHello world\nHello\r\nWorld\rAgain\r\n--X-BOUNDARY--\r\n";
    let stream = stream::iter(
        data.chars()
            .map(|ch| ch.to_string())
            .map(|part| multer::Result::Ok(Bytes::copy_from_slice(part.as_bytes()))),
    );

    let mut m = Multipart::new(stream, "X-BOUNDARY");

    while let Some((idx, field)) = m.next_field_with_idx().await.unwrap() {
        if idx == 0 {
            assert_eq!(field.name(), Some("My Field"));
            assert_eq!(field.file_name(), None);
            assert_eq!(field.content_type(), None);
            assert_eq!(field.index(), 0);

            assert_eq!(field.text().await, Ok("abcd".to_owned()));
        } else if idx == 1 {
            assert_eq!(field.name(), Some("File Field"));
            assert_eq!(field.file_name(), Some("a-text-file.txt"));
            assert_eq!(field.content_type(), Some(&mime::TEXT_PLAIN));
            assert_eq!(field.index(), 1);

            assert_eq!(field.text().await, Ok("Hello world\nHello\r\nWorld\rAgain".to_owned()));
        }
    }
}

#[tokio::test]
async fn test_multipart_empty() {
    let data = "--X-BOUNDARY--\r\n";
    let stream = stream::iter(
        data.chars()
            .map(|ch| ch.to_string())
            .map(|part| multer::Result::Ok(Bytes::copy_from_slice(part.as_bytes()))),
    );

    let mut m = Multipart::new(stream, "X-BOUNDARY");

    assert!(m.next_field().await.unwrap().is_none());
    assert!(m.next_field().await.unwrap().is_none());
}

#[tokio::test]
async fn test_multipart_clean_field() {
    let data = "--X-BOUNDARY\r\nContent-Disposition: form-data; name=\"My Field\"\r\n\r\nabcd\r\n--X-BOUNDARY\r\nContent-Disposition: form-data; name=\"File Field\"; filename=\"a-text-file.txt\"\r\nContent-Type: text/plain\r\n\r\nHello world\nHello\r\nWorld\rAgain\r\n--X-BOUNDARY--\r\n";
    let stream = stream::iter(
        data.chars()
            .map(|ch| ch.to_string())
            .map(|part| multer::Result::Ok(Bytes::copy_from_slice(part.as_bytes()))),
    );

    let mut m = Multipart::new(stream, "X-BOUNDARY");

    assert!(m.next_field().await.unwrap().is_some());
    assert!(m.next_field().await.unwrap().is_some());
    assert!(m.next_field().await.unwrap().is_none());
}
