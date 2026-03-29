//! Fuzz target: varint encode/decode round-trip and decode robustness.

#![no_main]
use libfuzzer_sys::fuzz_target;
use protoc_rs_runtime::wire;

fuzz_target!(|data: &[u8]| {
    // Decode must not panic
    if let Ok((value, consumed)) = wire::decode_varint(data) {
        // Re-encode and verify round-trip
        let mut buf = Vec::new();
        wire::encode_varint(value, &mut buf);
        let (decoded, _) = wire::decode_varint(&buf).expect("re-decode must succeed");
        assert_eq!(value, decoded, "varint round-trip mismatch");

        // Verify varint_len matches actual encoded length
        assert_eq!(buf.len(), wire::varint_len(value), "varint_len mismatch");
    }

    // Also fuzz fixed32/64 decode
    let _ = wire::decode_fixed32(data);
    let _ = wire::decode_fixed64(data);

    // Fuzz tag decode
    let _ = wire::decode_tag(data);
});
