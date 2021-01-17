#![no_main]

use std::convert::Infallible;

use multer::Multipart;
use multer::bytes::Bytes;
use futures_util::stream::once;
use libfuzzer_sys::fuzz_target;
use tokio::runtime;

fuzz_target!(|data: &[u8]| {
    let data = data.to_vec();
    let stream = once(async move { Result::<Bytes, Infallible>::Ok(Bytes::from(data)) });

    let mut multipart = Multipart::new(stream, "X-BOUNDARY");

    let rt = runtime::Builder::new_current_thread().build().expect("runtime");
    rt.block_on(async {
        let mut breaks = 0;
        while breaks < 3 {
            let field = multipart.next_field().await;
            match field {
                Err(_) | Ok(None) => breaks += 1,
                Ok(Some(_)) => continue,
            }
        }
    })
});
