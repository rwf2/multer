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

### Reading Fields

`next_field()` now returns a field as soon as its headers are complete.
`field.chunk()` can return part of a file before a later read fails or exceeds
a size limit. To check that the entire form was read successfully, continue
calling `next_field()` until it returns `Ok(None)`.

Multer stops reading at the closing boundary. `SizeLimit::whole_stream()`
counts the entire input chunk containing that boundary, including any bytes
that follow it.

If the input fails, ends before the field is complete, or exceeds a size
limit, `field.chunk()` returns the error once, then `Ok(None)`. After you drop
the field, `next_field()` returns the same error. Once `next_field()` reports
a terminal error, later calls return `Error::MultipartFailed`.

You can still read the next field after `Error::UnknownField` or a JSON
decoding error from `Field::json()`. If you get `Error::LockFailure` because
you still hold the previous field, drop it and call `next_field()` again.

### Stream Errors

The field and `Multipart` now share the cause of `Error::StreamReadFailed`.
If you construct this error from a `Box<dyn std::error::Error + Send + Sync>`,
wrap the box in an `Arc`.

```diff
- Error::StreamReadFailed(boxed)
+ Error::StreamReadFailed(std::sync::Arc::new(boxed))
```

When matching `Error::StreamReadFailed(cause)`, use `cause.downcast_ref::<T>()`
to borrow the original error. Calling `source()` on the `multer::Error` also
returns a reference to it. Move `cause` to keep it or use `Arc::clone(&cause)`
to share it.

`Arc::try_unwrap(cause)` returns the original box when you hold the last
`Arc`, or `Err(cause)` if the error is still shared. After a field read error,
`Multipart` holds an `Arc` until `next_field()` returns the error or you drop
the parser. Call `boxed.downcast::<T>()` on the recovered box to take ownership
of the concrete error.

### Field and Header Limits

* `Constraints::max_fields(n)` accepts at most `n` parts, counting empty,
  unnamed, skipped, and rejected parts. The default is unlimited.
* `SizeLimit::per_header(n)` limits the raw header section of each part to
  `n` bytes, including names, values, separators, and the final blank line.
  Multer checks this limit as it reads the headers. The default is `u64::MAX`.
* To limit the parsed value of a particular header, use `for_header()` or
  `try_for_header()`. If the header occurs more than once in a part, every
  value must fit. `Field::headers()` contains the last value for that name.

### Boundaries and Field Names

In 3.1, `parse_boundary()` used the first `boundary` parameter in
`Content-Type`. It now returns `Error::MultipleBoundaries` if the parameter
appears more than once, even when the values are identical.

The opening boundary must appear at the start of the input or immediately
after `\r\n`. Multer skips a boundary mentioned in the middle of a line. If
it reaches the end of the input without finding an opening boundary,
`next_field()` returns `Error::IncompleteStream`. For a boundary of `B`, a
body containing only `--B--` is a complete, empty form.

A part that begins with a blank line has no headers. Multer returns it as an
unnamed field with its own body.

Multer uses the first `name` and `filename` parameters in `Content-Disposition`.
It ignores ASCII case in parameter names and preserves case in their values.
An unquoted value ends at a space, tab, or semicolon.

```http
Content-Disposition: form-data; note="a name=x"; Name=Invoice; FileName=Memo.txt
```

Here, `field.name()` returns `Some("Invoice")` and `field.file_name()` returns
`Some("Memo.txt")`. The text `name=x` is part of the quoted `note` value.
Both `allowed_fields()` and `for_field()` use the name `"Invoice"`.

## Usage with [hyper.rs](https://hyper.rs/) server

An [example](https://github.com/rousan/multer-rs/blob/master/examples/hyper_server_example.rs) showing usage with [hyper.rs](https://hyper.rs/).

For more examples, please visit [examples](https://github.com/rousan/multer-rs/tree/master/examples).

## Contributing

Your PRs and suggestions are always welcome.
