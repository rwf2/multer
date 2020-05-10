use bytes::Bytes;
use futures::stream::{Stream, StreamExt};
use futures::TryStreamExt;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Request, Response, Server};
use multer::{Error, Field, Multipart};
use multer::{ErrorExt, ResultExt};
use std::{convert::Infallible, net::SocketAddr};
use tokio::fs::{File, OpenOptions};
use tokio::io::{AsyncWrite, AsyncWriteExt};

async fn handle(req: Request<Body>) -> Result<Response<Body>, Infallible> {
    let mut stream = req.into_body();
    // let mut stream = futures::stream::once(async move { Result::<&'static str, Infallible>::Ok("abc") });
    // let stream = futures::stream::iter(vec![
    //     Ok("abc"),
    //     Err(std::io::Error::new(std::io::ErrorKind::Other, "Heyyyyyyyyyy")),
    // ]);

    // while let Some(Ok(data)) = stream.next().await {
    //     print!("{:?}", String::from_utf8_lossy(&*data).to_string())
    // }

    // let reader =
    //     tokio::io::stream_reader(stream.map_err(|err| tokio::io::Error::new(tokio::io::ErrorKind::Other, err)));
    //
    // let mut multipart = Multipart::with_reader(reader, "X-INSOMNIA-BOUNDARY");

    let mut multipart = Multipart::new(stream, "X-INSOMNIA-BOUNDARY");

    while let Some(field) = multipart.next_field().await.unwrap() {
        println!("{:?}", field.name());
        // let text = field.text().await.unwrap();
        // println!("{}", text);
    }

    // let mut m = Multipart::new(stream, "X-INSOMNIA-BOUNDARY");
    //
    // // let mut i = 0;
    // 'outer: while let Some(field) = m.next_field().await.context("No field").unwrap() {
    //     // i += 1;
    //     println!("{:?}", field.headers());
    //
    //     // let mut file = OpenOptions::new()
    //     //     .create(true)
    //     //     .write(true)
    //     //     .open(format!("/Users/rousan/Downloads/{}.pdf", i))
    //     //     .await
    //     //     .unwrap();
    //     //
    //     // while let Some(chunk) = field.next().await {
    //     //     let chunk = chunk.expect("No chunk");
    //     //     file.write_all(&chunk).await.unwrap();
    //     // }
    //     //
    //     // file.flush().await.unwrap();
    //     //
    //     // let mut len = 0;
    //
    //     let name = field.name().unwrap();
    //
    //     if name == "my" {
    //         println!("Name: {}", name);
    //         let text = field.text().await.unwrap();
    //         println!("{}", text);
    //         continue;
    //     }
    //
    //     if name == "f abc" {
    //         println!("Name: {}F, Filename: {}", name, field.file_name().unwrap());
    //         let text = field.text().await.unwrap();
    //         println!("{}", text);
    //         continue;
    //     }
    //
    //     // if name == "file_bin" {
    //     //     println!("Name: {}, Filename: {}", name, field.file_name().unwrap());
    //     //     let bytes = field.bytes().await.unwrap();
    //     //     println!("bytes len: {}", bytes.len());
    //     //     continue;
    //     // }
    //
    //     // println!("{}", i);
    //     //
    //     // let mut len = 0;
    //     // while let Some(chunk) = field.chunk().await.context("No chunk")? {
    //     //     len += chunk.len();
    //     //     println!("Chunk Size: {}", chunk.len());
    //     //
    //     //     // if i == 2 {
    //     //     //     continue 'outer;
    //     //     // }
    //     // }
    //     // println!("Total field data size: {}", len);
    // }

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
