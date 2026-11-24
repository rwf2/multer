//! Run with `cargo bench --features tokio-io --bench small_chunks`.
//! For a release comparison, alternate the same benchmark on both revisions.
//! Compare throughput within each pair for each payload, adapter and chunk size.

use std::convert::Infallible;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Instant;

use bytes::Bytes;
use futures_util::stream;
use multer::Multipart;
use tokio::io::{AsyncRead, ReadBuf};

const PAYLOAD_LEN: usize = 1024 * 1024;
const ITERATIONS: usize = 1536;
const BOUNDARY: &str = "AuditForm123";

struct Reader<'r> {
    data: &'r [u8],
    chunk_size: usize,
}

impl AsyncRead for Reader<'_> {
    fn poll_read(mut self: Pin<&mut Self>, _: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<std::io::Result<()>> {
        let count = self.data.len().min(self.chunk_size).min(buf.remaining());
        let (chunk, rest) = self.data.split_at(count);
        buf.put_slice(chunk);
        self.data = rest;
        Poll::Ready(Ok(()))
    }
}

async fn read_form(mut form: Multipart<'_>, expected_sum: u64) {
    let mut field = form.next_field().await.unwrap().unwrap();
    assert_eq!(field.name(), Some("upload"));
    assert_eq!(field.file_name(), Some("ordinary.bin"));
    let mut len = 0;
    let mut sum = 0u64;
    while let Some(chunk) = field.chunk().await.unwrap() {
        len += chunk.len();
        sum += chunk.iter().map(|&byte| u64::from(byte)).sum::<u64>();
    }
    assert_eq!(len, PAYLOAD_LEN);
    assert_eq!(sum, expected_sum);
    drop(field);
    assert!(form.next_field().await.unwrap().is_none());
}

async fn measure<'r>(make_form: impl Fn() -> Multipart<'r>, expected_sum: u64) -> f64 {
    for _ in 0..50 {
        read_form(make_form(), expected_sum).await;
    }

    let start = Instant::now();
    for _ in 0..ITERATIONS {
        read_form(make_form(), expected_sum).await;
    }

    start.elapsed().as_nanos() as f64 / ITERATIONS as f64
}

async fn benchmark_payload(label: &str, tile: &[u8]) {
    let mut wire = format!(
        "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"upload\"; filename=\"ordinary.bin\"\r\n\
         Content-Type: application/octet-stream\r\n\r\n"
    )
    .into_bytes();
    let payload = tile.iter().copied().cycle().take(PAYLOAD_LEN);
    let expected_sum = payload.clone().map(u64::from).sum();
    wire.extend(payload);
    wire.extend_from_slice(format!("\r\n--{BOUNDARY}--\r\n").as_bytes());
    let wire = Bytes::from(wire);

    for chunk_size in [512, 1024] {
        let items: Vec<_> = (0..wire.len())
            .step_by(chunk_size)
            .map(|i| wire.slice(i..(i + chunk_size).min(wire.len())))
            .collect();
        let ns = measure(
            || {
                let source = stream::iter(items.iter().cloned().map(Ok::<_, Infallible>));
                Multipart::new(source, BOUNDARY)
            },
            expected_sum,
        )
        .await;
        println!("{label},stream,{chunk_size},{ITERATIONS},{ns:.1}");
        let ns = measure(
            || {
                Multipart::with_reader(
                    Reader {
                        data: &wire,
                        chunk_size,
                    },
                    BOUNDARY,
                )
            },
            expected_sum,
        )
        .await;
        println!("{label},reader,{chunk_size},{ITERATIONS},{ns:.1}");
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let binary: Vec<_> = (0..=u8::MAX).collect();
    let mut text =
        b"An ordinary report line with a value and a date. The next line is part of the same file.\r\n".repeat(18);
    text.extend_from_slice(b"\r\n--------------------\r\nReport section two\r\n");
    println!("payload,adapter,chunk_bytes,forms,ns_per_form");
    benchmark_payload("binary", &binary).await;
    benchmark_payload("text", &text).await;
}
