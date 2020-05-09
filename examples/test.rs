use futures::stream::{StreamExt, TryStreamExt};
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Request, Response, Server};
use multer::{Error, Field, Multipart};
use multer::{ErrorExt, ResultExt};
use std::{convert::Infallible, net::SocketAddr};
use tokio::fs::{File, OpenOptions};
use tokio::io::{AsyncWrite, AsyncWriteExt};

async fn handle(req: Request<Body>) -> Result<Response<Body>, Infallible> {
    let stream = req.into_body().map(|item| item.wrap());

    // while let Some(Ok(data)) = stream.next().await {
    //     print!("{:?}", String::from_utf8_lossy(&*data).to_string())
    // }
    //
    // println!();

    let mut m = Multipart::new(stream, "X-INSOMNIA-BOUNDARY");

    let mut i = 0;
    while let Some(field) = m.next().await {
        let mut field = field.expect("No field");
        println!("{:?}", field.headers());

        println!("")

        // let mut file = OpenOptions::new()
        //     .create(true)
        //     .write(true)
        //     .open(format!("/Users/rousan/Downloads/{}.pdf", i))
        //     .await
        //     .unwrap();
        //
        // while let Some(chunk) = field.next().await {
        //     let chunk = chunk.expect("No chunk");
        //     file.write_all(&chunk).await.unwrap();
        // }
        //
        // file.flush().await.unwrap();
        //
        // // let mut len = 0;
        // // while let Some(chunk) = field.next().await {
        // //     let chunk = chunk.expect("No chunk");
        // //     len += chunk.len();
        // //     // println!("Chunk Size: {}", chunk.len());
        // // }
        // // println!("Total field data size: {}", len);
        // i += 1;
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
