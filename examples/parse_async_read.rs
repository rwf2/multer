use tokio::io::AsyncRead;
// Import multer types.
use multer::Multipart;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Generate an `AsyncRead` and the boundary from somewhere e.g. server request body.
    let (reader, boundary) = get_async_reader_from_somewhere().await;

    // Create a `Multipart` instance from that async reader and the boundary.
    let mut multipart = Multipart::with_reader(reader, boundary);

    // Iterate over the fields, use `next_field()` to get the next field.
    while let Some(field) = multipart.next_field().await? {
        // Get field name.
        let name = field.name();
        // Get the field's filename if provided in "Content-Disposition" header.
        let file_name = field.file_name();

        println!("Name: {:?}, File Name: {:?}", name, file_name);

        // Read field content as text.
        let content = field.text().await?;
        println!("Content: {:?}", content);
    }

    Ok(())
}

// Generate an `AsyncRead` and the boundary from somewhere e.g. server request body.
async fn get_async_reader_from_somewhere() -> (impl AsyncRead, &'static str) {
    let data = "--X-BOUNDARY\r\nContent-Disposition: form-data; name=\"My Field\"\r\n\r\nabcd\r\n--X-BOUNDARY\r\nContent-Disposition: form-data; name=\"File Field\"; filename=\"a-text-file.txt\"\r\nContent-Type: text/plain\r\n\r\nHello world\nHello\r\nWorld\rAgain\r\n--X-BOUNDARY--\r\n";

    (data.as_bytes(), "X-BOUNDARY")
}
