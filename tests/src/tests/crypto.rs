use super::*;

#[test]
fn crypto_key_size_matches_firmware() {
    let source = include_str!("../../../src/bin/crypto/mod.rs");
    assert!(
        source.contains("pub const KEY_SIZE: usize = 32;"),
        "tests/src/lib.rs mirrors KEY_SIZE; update it when the firmware changes"
    );
    assert_eq!(KEY_SIZE, 32);
}
