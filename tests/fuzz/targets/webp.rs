#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = keine_media::decode_webp(data, |size| size);
});
