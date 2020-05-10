use hyper::{header::CONTENT_TYPE, Body, Request, Response, Server, StatusCode};
// Import the routerify prelude traits.
use routerify::prelude::*;
// Import routerify types.
use routerify::{Middleware, Router, RouterService};
use std::{convert::Infallible, net::SocketAddr};
// Import the multer types.
use multer::Multipart;

// A handler to handle file upload at `/upload` path.
async fn file_upload_handler(req: Request<Body>) -> Result<Response<Body>, Infallible> {
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
            .body(Body::from("BAD REQUEST"))
            .unwrap());
    }

    // Process the multipart e.g. you can store them in files.
    if let Err(err) = process_multipart(req.into_body(), boundary.unwrap()).await {
        return Ok(Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(Body::from(format!("INTERNAL SERVER ERROR: {}", err)))
            .unwrap());
    }

    Ok(Response::new(Body::from("Success")))
}

// Process the request body as multipart/form-data.
async fn process_multipart(body: Body, boundary: String) -> multer::Result<()> {
    // Create a Multipart instance from the request body.
    let mut multipart = Multipart::new(body, boundary);

    // Iterate over the fields, `next_field` method will return the next field if available.
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

// A routerify middleware which logs an http request.
async fn logger(req: Request<Body>) -> Result<Request<Body>, Infallible> {
    println!("{} {} {}", req.remote_addr(), req.method(), req.uri().path());
    Ok(req)
}

// Create a `Router<Body, Infallible>` for response body type `hyper::Body` and for handler error type `Infallible`.
fn router() -> Router<Body, Infallible> {
    // Create a router and specify the logger middleware and the handlers.
    // Here, "Middleware::pre" means we're adding a pre middleware which will be executed
    // before any route handlers.
    Router::builder()
        .middleware(Middleware::pre(logger))
        .post("/upload", file_upload_handler)
        .build()
        .unwrap()
}

#[tokio::main]
async fn main() {
    let router = router();

    // Create a Service from the router above to handle incoming requests.
    let service = RouterService::new(router).unwrap();

    // The address on which the server will be listening.
    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));

    // Create a server by passing the created service to `.serve` method.
    let server = Server::bind(&addr).serve(service);

    println!("App is running on: {}", addr);
    if let Err(err) = server.await {
        eprintln!("Server error: {}", err);
    }
}
