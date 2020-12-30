use bytes::Bytes;
use futures::stream;
use multer::{Constraints, Multipart, SizeLimit};

#[tokio::test]
async fn test_multipart_basic() {
    let data = "--X-BOUNDARY\r\nContent-Disposition: form-data; name=\"my_text_field\"\r\n\r\nabcd\r\n--X-BOUNDARY\r\nContent-Disposition: form-data; name=\"my_file_field\"; filename=\"a-text-file.txt\"\r\nContent-Type: text/plain\r\n\r\nHello world\nHello\r\nWorld\rAgain\r\n--X-BOUNDARY--\r\n";
    let stream = stream::iter(
        data.chars()
            .map(|ch| ch.to_string())
            .map(|part| multer::Result::Ok(Bytes::copy_from_slice(part.as_bytes()))),
    );

    let mut m = Multipart::new(stream, "X-BOUNDARY");

    while let Some((idx, field)) = m.next_field_with_idx().await.unwrap() {
        if idx == 0 {
            assert_eq!(field.name(), Some("my_text_field"));
            assert_eq!(field.file_name(), None);
            assert_eq!(field.content_type(), None);
            assert_eq!(field.index(), 0);

            assert_eq!(field.text().await, Ok("abcd".to_owned()));
        } else if idx == 1 {
            assert_eq!(field.name(), Some("my_file_field"));
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
    let data = "--X-BOUNDARY\r\nContent-Disposition: form-data; name=\"my_text_field\"\r\n\r\nabcd\r\n--X-BOUNDARY\r\nContent-Disposition: form-data; name=\"my_file_field\"; filename=\"a-text-file.txt\"\r\nContent-Type: text/plain\r\n\r\nHello world\nHello\r\nWorld\rAgain\r\n--X-BOUNDARY--\r\n";
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

#[tokio::test]
async fn test_multipart_header() {
    let should_pass = [
        "ignored header\r\n--X-BOUNDARY\r\nContent-Disposition: form-data; name=\"my_text_field\"\r\n\r\nabcd\r\n--X-BOUNDARY--\r\n",
        "\r\nignored header\r\n--X-BOUNDARY\r\nContent-Disposition: form-data; name=\"my_text_field\"\r\n\r\nabcd\r\n--X-BOUNDARY--\r\n",
        "\r\n--X-BOUNDARY\r\nContent-Disposition: form-data; name=\"my_text_field\"\r\n\r\nabcd\r\n--X-BOUNDARY--\r\n",
        "\r\n\r\n--X-BOUNDARY\r\nContent-Disposition: form-data; name=\"my_text_field\"\r\n\r\nabcd\r\n--X-BOUNDARY--\r\n",
    ];

    for data in should_pass.iter() {
        let stream = stream::iter(
            data.chars()
                .map(|ch| ch.to_string())
                .map(|part| multer::Result::Ok(Bytes::copy_from_slice(part.as_bytes()))),
        );

        let mut m = Multipart::new(stream, "X-BOUNDARY");

        assert_eq!(
            m.next_field().await.unwrap().unwrap().text().await.unwrap(),
            "abcd".to_owned()
        );
    }
}

#[tokio::test]
async fn test_multipart_constraint_allowed_fields_normal() {
    let data = "--X-BOUNDARY\r\nContent-Disposition: form-data; name=\"my_text_field\"\r\n\r\nabcd\r\n--X-BOUNDARY\r\nContent-Disposition: form-data; name=\"my_file_field\"; filename=\"a-text-file.txt\"\r\nContent-Type: text/plain\r\n\r\nHello world\nHello\r\nWorld\rAgain\r\n--X-BOUNDARY--\r\n";
    let stream = stream::iter(
        data.chars()
            .map(|ch| ch.to_string())
            .map(|part| multer::Result::Ok(Bytes::copy_from_slice(part.as_bytes()))),
    );

    let constraints = Constraints::new().allowed_fields(vec!["my_text_field", "my_file_field"]);
    let mut m = Multipart::new_with_constraints(stream, "X-BOUNDARY", constraints);

    assert_eq!(
        m.next_field().await.unwrap().unwrap().text().await.unwrap(),
        "abcd".to_owned()
    );
    assert_eq!(
        m.next_field().await.unwrap().unwrap().text().await.unwrap(),
        "Hello world\nHello\r\nWorld\rAgain".to_owned()
    );
}

#[tokio::test]
#[should_panic]
async fn test_multipart_constraint_allowed_fields_unknown_field() {
    let data = "--X-BOUNDARY\r\nContent-Disposition: form-data; name=\"my_text_field\"\r\n\r\nabcd\r\n--X-BOUNDARY\r\nContent-Disposition: form-data; name=\"my_file_field\"; filename=\"a-text-file.txt\"\r\nContent-Type: text/plain\r\n\r\nHello world\nHello\r\nWorld\rAgain\r\n--X-BOUNDARY--\r\n";
    let stream = stream::iter(
        data.chars()
            .map(|ch| ch.to_string())
            .map(|part| multer::Result::Ok(Bytes::copy_from_slice(part.as_bytes()))),
    );

    let constraints = Constraints::new().allowed_fields(vec!["my_text_field"]);
    let mut m = Multipart::new_with_constraints(stream, "X-BOUNDARY", constraints);

    assert!(m.next_field().await.unwrap().is_some());
    assert!(m.next_field().await.unwrap().is_some());
    assert!(m.next_field().await.unwrap().is_none());
}

#[tokio::test]
async fn test_multipart_constraint_size_limit_whole_stream() {
    let data = "--X-BOUNDARY\r\nContent-Disposition: form-data; name=\"my_text_field\"\r\n\r\nabcd\r\n--X-BOUNDARY\r\nContent-Disposition: form-data; name=\"my_file_field\"; filename=\"a-text-file.txt\"\r\nContent-Type: text/plain\r\n\r\nHello world\nHello\r\nWorld\rAgain\r\n--X-BOUNDARY--\r\n";
    let stream = stream::iter(
        data.chars()
            .map(|ch| ch.to_string())
            .map(|part| multer::Result::Ok(Bytes::copy_from_slice(part.as_bytes()))),
    );

    let constraints = Constraints::new()
        .allowed_fields(vec!["my_text_field", "my_file_field"])
        .size_limit(SizeLimit::new().whole_stream(248));

    let mut m = Multipart::new_with_constraints(stream, "X-BOUNDARY", constraints);

    assert_eq!(
        m.next_field().await.unwrap().unwrap().text().await.unwrap(),
        "abcd".to_owned()
    );
    assert_eq!(
        m.next_field().await.unwrap().unwrap().text().await.unwrap(),
        "Hello world\nHello\r\nWorld\rAgain".to_owned()
    );
}

#[tokio::test]
#[should_panic]
async fn test_multipart_constraint_size_limit_whole_stream_size_exceeded() {
    let data = "--X-BOUNDARY\r\nContent-Disposition: form-data; name=\"my_text_field\"\r\n\r\nabcd\r\n--X-BOUNDARY\r\nContent-Disposition: form-data; name=\"my_file_field\"; filename=\"a-text-file.txt\"\r\nContent-Type: text/plain\r\n\r\nHello world\nHello\r\nWorld\rAgain\r\n--X-BOUNDARY--\r\n";
    let stream = stream::iter(
        data.chars()
            .map(|ch| ch.to_string())
            .map(|part| multer::Result::Ok(Bytes::copy_from_slice(part.as_bytes()))),
    );

    let constraints = Constraints::new()
        .allowed_fields(vec!["my_text_field", "my_file_field"])
        .size_limit(SizeLimit::new().whole_stream(100));

    let mut m = Multipart::new_with_constraints(stream, "X-BOUNDARY", constraints);

    assert!(m.next_field().await.unwrap().is_some());
    assert!(m.next_field().await.unwrap().is_some());
    assert!(m.next_field().await.unwrap().is_none());
}

#[tokio::test]
async fn test_multipart_constraint_size_limit_per_field() {
    let data = "--X-BOUNDARY\r\nContent-Disposition: form-data; name=\"my_text_field\"\r\n\r\nabcd\r\n--X-BOUNDARY\r\nContent-Disposition: form-data; name=\"my_file_field\"; filename=\"a-text-file.txt\"\r\nContent-Type: text/plain\r\n\r\nHello world\nHello\r\nWorld\rAgain\r\n--X-BOUNDARY--\r\n";
    let stream = stream::iter(
        data.chars()
            .map(|ch| ch.to_string())
            .map(|part| multer::Result::Ok(Bytes::copy_from_slice(part.as_bytes()))),
    );

    let constraints = Constraints::new()
        .allowed_fields(vec!["my_text_field", "my_file_field"])
        .size_limit(SizeLimit::new().whole_stream(248).per_field(100));

    let mut m = Multipart::new_with_constraints(stream, "X-BOUNDARY", constraints);

    assert_eq!(
        m.next_field().await.unwrap().unwrap().text().await.unwrap(),
        "abcd".to_owned()
    );
    assert_eq!(
        m.next_field().await.unwrap().unwrap().text().await.unwrap(),
        "Hello world\nHello\r\nWorld\rAgain".to_owned()
    );
}

#[tokio::test]
#[should_panic]
async fn test_multipart_constraint_size_limit_per_field_size_exceeded() {
    let data = "--X-BOUNDARY\r\nContent-Disposition: form-data; name=\"my_text_field\"\r\n\r\nabcd\r\n--X-BOUNDARY\r\nContent-Disposition: form-data; name=\"my_file_field\"; filename=\"a-text-file.txt\"\r\nContent-Type: text/plain\r\n\r\nHello world\nHello\r\nWorld\rAgain\r\n--X-BOUNDARY--\r\n";
    let stream = stream::iter(
        data.chars()
            .map(|ch| ch.to_string())
            .map(|part| multer::Result::Ok(Bytes::copy_from_slice(part.as_bytes()))),
    );

    let constraints = Constraints::new()
        .allowed_fields(vec!["my_text_field", "my_file_field"])
        .size_limit(SizeLimit::new().whole_stream(248).per_field(10));

    let mut m = Multipart::new_with_constraints(stream, "X-BOUNDARY", constraints);

    assert!(m.next_field().await.unwrap().is_some());
    assert!(m.next_field().await.unwrap().is_some());
    assert!(m.next_field().await.unwrap().is_none());
}

#[tokio::test]
async fn test_multipart_constraint_size_limit_for_field() {
    let data = "--X-BOUNDARY\r\nContent-Disposition: form-data; name=\"my_text_field\"\r\n\r\nabcd\r\n--X-BOUNDARY\r\nContent-Disposition: form-data; name=\"my_file_field\"; filename=\"a-text-file.txt\"\r\nContent-Type: text/plain\r\n\r\nHello world\nHello\r\nWorld\rAgain\r\n--X-BOUNDARY--\r\n";
    let stream = stream::iter(
        data.chars()
            .map(|ch| ch.to_string())
            .map(|part| multer::Result::Ok(Bytes::copy_from_slice(part.as_bytes()))),
    );

    let constraints = Constraints::new()
        .allowed_fields(vec!["my_text_field", "my_file_field"])
        .size_limit(
            SizeLimit::new()
                .whole_stream(248)
                .per_field(100)
                .for_field("my_text_field", 4)
                .for_field("my_file_field", 30),
        );

    let mut m = Multipart::new_with_constraints(stream, "X-BOUNDARY", constraints);

    assert_eq!(
        m.next_field().await.unwrap().unwrap().text().await.unwrap(),
        "abcd".to_owned()
    );
    assert_eq!(
        m.next_field().await.unwrap().unwrap().text().await.unwrap(),
        "Hello world\nHello\r\nWorld\rAgain".to_owned()
    );
}

#[tokio::test]
#[should_panic]
async fn test_multipart_constraint_size_limit_for_field_size_exceeded() {
    let data = "--X-BOUNDARY\r\nContent-Disposition: form-data; name=\"my_text_field\"\r\n\r\nabcd\r\n--X-BOUNDARY\r\nContent-Disposition: form-data; name=\"my_file_field\"; filename=\"a-text-file.txt\"\r\nContent-Type: text/plain\r\n\r\nHello world\nHello\r\nWorld\rAgain\r\n--X-BOUNDARY--\r\n";
    let stream = stream::iter(
        data.chars()
            .map(|ch| ch.to_string())
            .map(|part| multer::Result::Ok(Bytes::copy_from_slice(part.as_bytes()))),
    );

    let constraints = Constraints::new()
        .allowed_fields(vec!["my_text_field", "my_file_field"])
        .size_limit(
            SizeLimit::new()
                .whole_stream(248)
                .per_field(100)
                .for_field("my_text_field", 4)
                .for_field("my_file_field", 10),
        );

    let mut m = Multipart::new_with_constraints(stream, "X-BOUNDARY", constraints);

    assert!(m.next_field().await.unwrap().is_some());
    assert!(m.next_field().await.unwrap().is_some());
    assert!(m.next_field().await.unwrap().is_none());
}
