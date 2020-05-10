[![Github Actions Status](https://github.com/rousan/multer-rs/workflows/Test/badge.svg)](https://github.com/rousan/multer-rs/actions)
[![crates.io](https://img.shields.io/crates/v/multer.svg)](https://crates.io/crates/multer)
[![Documentation](https://docs.rs/multer/badge.svg)](https://docs.rs/multer)
[![MIT](https://img.shields.io/crates/l/multer.svg)](./LICENSE)

# multer-rs

An async parser for `multipart/form-data` content-type in Rust.

It accepts a [`Stream`](https://docs.rs/futures/0.3.5/futures/stream/trait.Stream.html) of [`Bytes`](https://docs.rs/bytes/0.5.4/bytes/struct.Bytes.html) as
a source, so that It can be plugged into any async Rust environment e.g. any async server.

[Docs](https://docs.rs/multer)

## Install    

Add this to your `Cargo.toml`:

```toml
[dependencies]
multer = "1.0"
```

# Basic Example

```rust
use bytes::Bytes;
use futures::stream::Stream;
// Import multer types.
use multer::Multipart;
use std::convert::Infallible;
use futures::stream::once;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Generate a byte stream and the boundary from somewhere e.g. server request body.
    let (stream, boundary) = get_byte_stream_from_somewhere().await;

    // Create a `Multipart` instance from that byte stream and the boundary.
    let mut multipart = Multipart::new(stream, boundary);

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

// Generate a byte stream and the boundary from somewhere e.g. server request body.
async fn get_byte_stream_from_somewhere() -> (impl Stream<Item = Result<Bytes, Infallible>>, &'static str) {
    let data = "--X-BOUNDARY\r\nContent-Disposition: form-data; name=\"My Field\"\r\n\r\nabcd\r\n--X-BOUNDARY--\r\n";
    let stream = once(async move { Result::<Bytes, Infallible>::Ok(Bytes::from(data)) });
    
    (stream, "X-BOUNDARY")
}
```

## Usage with [hyper.rs](https://hyper.rs/) server

An [example](https://github.com/rousan/multer-rs/blob/master/examples/hyper_server_example.rs) showing usage with [hyper.rs](https://hyper.rs/).

For more examples, please visit [examples](https://github.com/rousan/multer-rs/tree/master/examples).

## Contributing

Your PRs and suggestions are always welcome.
