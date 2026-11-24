//! Run with `cargo bench --features tokio-io --bench small_chunks`.
//! For a release comparison, alternate the same benchmark on both revisions.
//! Group results by adapter and chunk size and compare throughput within each pair.

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

async fn read_form(mut form: Multipart<'_>) {
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
    assert_eq!(sum, (PAYLOAD_LEN / 256) as u64 * (0..=255u64).sum::<u64>());
    drop(field);
    assert!(form.next_field().await.unwrap().is_none());
}

async fn measure<'r>(adapter: &str, chunk_size: usize, make_form: impl Fn() -> Multipart<'r>) {
    for _ in 0..50 {
        read_form(make_form()).await;
    }

    let start = Instant::now();
    for _ in 0..ITERATIONS {
        read_form(make_form()).await;
    }

    let ns_per_form = start.elapsed().as_nanos() as f64 / ITERATIONS as f64;
    println!("{adapter},{chunk_size},{ITERATIONS},{ns_per_form:.1}");
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let mut wire = format!(
        "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"upload\"; filename=\"ordinary.bin\"\r\n\
         Content-Type: application/octet-stream\r\n\r\n"
    )
    .into_bytes();
    wire.extend((0..=u8::MAX).cycle().take(PAYLOAD_LEN));
    wire.extend_from_slice(format!("\r\n--{BOUNDARY}--\r\n").as_bytes());
    let wire = Bytes::from(wire);

    println!("adapter,chunk_bytes,forms,ns_per_form");
    for chunk_size in [512, 1024] {
        let items: Vec<_> = (0..wire.len())
            .step_by(chunk_size)
            .map(|i| wire.slice(i..(i + chunk_size).min(wire.len())))
            .collect();
        measure("stream", chunk_size, || {
            let source = stream::iter(items.iter().cloned().map(Ok::<_, Infallible>));
            Multipart::new(source, BOUNDARY)
        })
        .await;
        measure("reader", chunk_size, || {
            Multipart::with_reader(
                Reader {
                    data: &wire,
                    chunk_size,
                },
                BOUNDARY,
            )
        })
        .await;
    }
}
