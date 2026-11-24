[![GitHub Actions Status](https://github.com/rousan/multer-rs/actions/workflows/test.yml/badge.svg)](https://github.com/rousan/multer-rs/actions)
[![crates.io](https://img.shields.io/crates/v/multer.svg)](https://crates.io/crates/multer)
[![Documentation](https://docs.rs/multer/badge.svg)](https://docs.rs/multer)
[![MIT](https://img.shields.io/crates/l/multer.svg)](./LICENSE)

# multer-rs

An async parser for `multipart/form-data` content-type in Rust.

It accepts a [`Stream`](https://docs.rs/futures/0.3/futures/stream/trait.Stream.html) of [`Bytes`](https://docs.rs/bytes/1/bytes/struct.Bytes.html) as
a source, so that It can be plugged into any async Rust environment e.g. any async server.

[Docs](https://docs.rs/multer)

## Install    

Add this to your `Cargo.toml`:

```toml
[dependencies]
multer = "4"
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
    while let Some(mut field) = multipart.next_field().await? {
        // Get field name.
        let name = field.name();
        // Get the field's filename if provided in "Content-Disposition" header.
        let file_name = field.file_name();

        println!("Name: {:?}, File Name: {:?}", name, file_name);

        // Process the field data chunks e.g. store them in a file.
        while let Some(chunk) = field.chunk().await? {
            // Do something with field chunk.
            println!("Chunk: {:?}", chunk);
        }
    }

    Ok(())
}

// Generate a byte stream and the boundary from somewhere e.g. server request body.
async fn get_byte_stream_from_somewhere() -> (impl Stream<Item = Result<Bytes, Infallible>>, &'static str) {
    let data = "--X-BOUNDARY\r\nContent-Disposition: form-data; name=\"my_text_field\"\r\n\r\nabcd\r\n--X-BOUNDARY--\r\n";
    let stream = once(async move { Result::<Bytes, Infallible>::Ok(Bytes::from(data)) });
    
    (stream, "X-BOUNDARY")
}
``` 

## Prevent Denial of Service (DoS) Attacks

This crate also provides some APIs to prevent potential DoS attacks with fine grained control. It's recommended to add some constraints
on field (specially text field) size to prevent DoS attacks exhausting the server's memory.

An example:

```rust
use multer::{Multipart, Constraints, SizeLimit};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create some constraints to be applied to the fields to prevent DoS attack.
    let constraints = Constraints::new()
         // We only accept `my_text_field` and `my_file_field` fields,
         // For any unknown field, we will throw an error.
         .allowed_fields(vec!["my_text_field", "my_file_field"])
         .size_limit(
             SizeLimit::new()
                 // Set 15mb as size limit for the whole stream body.
                 .whole_stream(15 * 1024 * 1024)
                 // Set 10mb as size limit for all fields.
                 .per_field(10 * 1024 * 1024)
                 // Set 30kb as size limit for our text field only.
                 .for_field("my_text_field", 30 * 1024),
         );

    // Create a `Multipart` instance from a stream and the constraints.
    let mut multipart = Multipart::with_constraints(some_stream, "X-BOUNDARY", constraints);

    while let Some(field) = multipart.next_field().await.unwrap() {
        let content = field.text().await.unwrap();
        assert_eq!(content, "abcd");
    } 
   
    Ok(())
}
```

## Migrating from 3.1

Fields and payload chunks can arrive earlier in 4.0. A later call can report a
source or size error after a payload prefix has been delivered. Consume each
field as a stream and drive `next_field()` through `Ok(None)` to confirm the
entire form. A fetched stream item counts in full toward `whole_stream`,
including its epilogue. The closing boundary completes parsing immediately.

A field reports a terminal read error once and then ends. Drop the field to
receive the same typed error from its parent. After the parent reports a
terminal error, further calls return `Error::MultipartFailed`. A live-field
`LockFailure`, an `UnknownField` rejection and a JSON conversion error after a
complete body still allow iteration to continue.

`Error::StreamReadFailed(cause)` now holds an
`Arc<Box<dyn std::error::Error + Send + Sync>>`. Both
`cause.downcast_ref::<T>()` and a downcast through Multer's `Error::source()`
reach the original error. Move the handle or use `Arc::clone(&cause)` to keep
it. To recover ownership, call `Arc::try_unwrap(cause)`, then
`boxed.downcast::<T>()`. While another report holds the cause, `try_unwrap`
returns that handle in `Err`. The parser releases its report when the parent
takes it or the shared state is dropped. To construct a stream error from an
existing boxed cause, use `Error::StreamReadFailed(Arc::new(boxed))`.

`Constraints::max_fields(n)` admits up to `n` parts, including empty,
unnamed, rejected and skipped parts. Configure `SizeLimit::per_header(n)` to
bound the raw header section of each part, including separators and its final
blank line. A section exactly at the limit is accepted. `for_header` and
`try_for_header` separately limit the retained parsed value for a named
header. The default permits any part count and header size representable by
the counters.

The opening boundary must start at byte zero or immediately after a CRLF.
Multer skips boundary-like text elsewhere in the preamble. Reaching transport EOF without
a marker at a line start produces `Error::IncompleteStream`. A correctly
positioned closing marker still completes an empty form.

A part beginning with a blank line yields an unnamed field with empty
headers. Content-Disposition parameters are read in order, so quoted text in
another parameter cannot supply a field's name or filename. Name-based
filtering and size limits use the parsed field name. The `name` and `filename`
parameter names are ASCII case-insensitive. Their values preserve case, and an
unquoted value ends at a space, tab or semicolon.

## Usage with [hyper.rs](https://hyper.rs/) server

An [example](https://github.com/rousan/multer-rs/blob/master/examples/hyper_server_example.rs) showing usage with [hyper.rs](https://hyper.rs/).

For more examples, please visit [examples](https://github.com/rousan/multer-rs/tree/master/examples).

## Contributing

Your PRs and suggestions are always welcome.
