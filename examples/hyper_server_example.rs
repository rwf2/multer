use std::{convert::Infallible, net::SocketAddr};

use bytes::Bytes;
use futures_util::StreamExt;
use http_body_util::{BodyStream, Full};
use hyper::{body::Incoming, header::CONTENT_TYPE, Request, Response, StatusCode};
// Import the multer types.
use multer::Multipart;

// A handler for incoming requests.
async fn handle(req: Request<Incoming>) -> Result<Response<Full<Bytes>>, Infallible> {
    // Extract the `multipart/form-data` boundary from the headers.
    let boundary = req
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|ct| ct.to_str().ok())
        .and_then(|ct| multer::parse_boundary(ct).ok());

    // Send `BAD_REQUEST` status if the content-type is not multipart/form-data.
    if boundary.is_none() {
        return Ok(Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .body(Full::from("BAD REQUEST"))
            .unwrap());
    }

    // Process the multipart e.g. you can store them in files.
    if let Err(err) = process_multipart(req.into_body(), boundary.unwrap()).await {
        return Ok(Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(Full::from(format!("INTERNAL SERVER ERROR: {}", err)))
            .unwrap());
    }

    Ok(Response::new(Full::from("Success")))
}

// Process the request body as multipart/form-data.
async fn process_multipart(body: Incoming, boundary: String) -> multer::Result<()> {
    // Convert the body into a stream of data frames.
    let body_stream = BodyStream::new(body)
        .filter_map(|result| async move { result.map(|frame| frame.into_data().ok()).transpose() });

    // Create a Multipart instance from the request body.
    let mut multipart = Multipart::new(body_stream, boundary);

    // Iterate over the fields, `next_field` method will return the next field if
    // available.
    while let Some(mut field) = multipart.next_field().await? {
        // Get the field name.
        let name = field.name();

        // Get the field's filename if provided in "Content-Disposition" header.
        let file_name = field.file_name();

        // Get the "Content-Type" header as `mime::Mime` type.
        let content_type = field.content_type();

        println!(
            "Name: {:?}, FileName: {:?}, Content-Type: {:?}",
            name, file_name, content_type
        );

        // Process the field data chunks e.g. store them in a file.
        let mut field_bytes_len = 0;
        while let Some(field_chunk) = field.chunk().await? {
            // Do something with field chunk.
            field_bytes_len += field_chunk.len();
        }

        println!("Field Bytes Length: {:?}", field_bytes_len);
    }

    Ok(())
}

#[tokio::main]
async fn main() {
    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    println!("Server running at: {}", addr);

    let service = hyper::service::service_fn(handle);

    loop {
        let (socket, _remote_addr) = listener.accept().await.unwrap();
        let socket = hyper_util::rt::TokioIo::new(socket);
        tokio::spawn(async move {
            if let Err(e) = hyper::server::conn::http1::Builder::new()
                .serve_connection(socket, service)
                .await
            {
                eprintln!("server error: {}", e);
            }
        });
    }
}
