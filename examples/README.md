# Examples of using multer-rs

These examples show of how to do common tasks using `multer-rs`.

Please visit: [Docs](https://docs.rs/multer) for the documentation.

Run an example:

```sh
 cargo run --example example_name
```

* [`simple_example`](simple_example.rs) - A basic example using `multer`.

* [`hyper_server_example`](hyper_server_example.rs) - Shows how to use this crate with Rust HTTP server [hyper](https://hyper.rs/).

* [`routerify_example`](routerify_example.rs) - Shows how to use this crate with [hyper](https://hyper.rs/) router implementation [Routerify](https://github.com/routerify/routerify).

* [`parse_async_read`](parse_async_read.rs) - Shows how to parse `multipart/form-data` from an [`AsyncRead`](https://docs.rs/tokio/0.2.20/tokio/io/trait.AsyncRead.html).

* [`prevent_ddos_attack`](prevent_ddos_attack.rs) - Shows how to apply some rules to prevent potential DDoS attack while handling `multipart/form-data`.