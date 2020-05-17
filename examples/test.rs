use bytes::Bytes;
use futures::stream::{Stream, StreamExt};
use futures::TryStreamExt;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Request, Response, Server};
use multer::{Constraints, Error, Field, Multipart, SizeLimit};
use std::{convert::Infallible, net::SocketAddr};
use tokio::fs::{File, OpenOptions};
use tokio::io::{AsyncWrite, AsyncWriteExt};

async fn handle(req: Request<Body>) -> Result<Response<Body>, Infallible> {
    let mut stream = req.into_body();

    let multipart_constraints = Constraints::new()
        .allowed_fields(vec!["a", "b"])
        .size_limit(SizeLimit::new().per_field(30).for_field("a", 10));

    let mut multipart = Multipart::new_with_constraints(stream, "X-INSOMNIA-BOUNDARY", multipart_constraints);

    while let Some(field) = multipart.next_field().await.unwrap() {
        println!("{:?}", field.name());
        let text = field.text().await.unwrap();
        println!("{}", text);
    }

    Ok(Response::new("Hello, World!".into()))
}

#[tokio::main]
async fn main() {
    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));

    let make_svc = make_service_fn(|_conn| async { Ok::<_, Infallible>(service_fn(handle)) });

    let server = Server::bind(&addr).serve(make_svc);

    println!("Server is running at: {}", addr);
    if let Err(e) = server.await {
        eprintln!("server error: {}", e);
    }
}
