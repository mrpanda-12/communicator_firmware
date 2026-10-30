use super::*;

#[test]
fn hex_device_id_formats_all_bytes() {
    let mut device = [0u8; 16];
    device[0] = 0x00;
    device[1] = 0x0F;
    device[15] = 0xFF;
    let formatted = hex_device_id(&device);
    assert_eq!(formatted.len(), DEVICE_ID_CHARS);
    assert!(formatted.starts_with("000F"));
    assert!(formatted.ends_with("FF"));
}

#[test]
fn write_hex_supports_separators() {
    let mut plain = String::<8>::new();
    write_hex(&mut plain, &[0x0A, 0xFF], None).unwrap();
    assert_eq!(plain.as_str(), "0AFF");

    let mut separated = String::<8>::new();
    write_hex(&mut separated, &[0x0A, 0xFF], Some(':')).unwrap();
    assert_eq!(separated.as_str(), "0A:FF");
}

#[test]
fn write_hex_empty_and_capacity() {
    let mut empty = String::<4>::new();
    write_hex(&mut empty, &[], None).unwrap();
    assert_eq!(empty.as_str(), "");

    let mut too_small = String::<3>::new();
    assert!(write_hex(&mut too_small, &[0xAA, 0xBB], None).is_err());
}
