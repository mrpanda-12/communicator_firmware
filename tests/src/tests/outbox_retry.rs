use super::*;

#[test]
fn outbox_lifecycle() {
    assert_eq!(outbox::MAX_OUTBOX_SLOTS, 2);

    let mut one = Vec::<PacketData, MAX_FRAGMENT_COUNT>::new();
    assert!(one.push(packet(b"one")).is_ok());

    // Broadcast-style sends clear themselves once fully transmitted.
    outbox::submit(0x100, one.clone(), false).unwrap();
    assert_eq!(outbox::pump(0, true, |_| true), 0);

    // Acknowledged sends wait for a retry.
    let mut two = Vec::<PacketData, MAX_FRAGMENT_COUNT>::new();
    assert!(two.push(packet(b"two")).is_ok());
    outbox::submit(0x200, two.clone(), true).unwrap();
    let mut sends = 0;
    assert_eq!(
        outbox::pump(0, true, |_| {
            sends += 1;
            true
        }),
        1
    );
    assert_eq!(sends, 1);

    // Before the retry interval nothing is resent.
    sends = 0;
    assert_eq!(
        outbox::pump(19_999_999, true, |_| {
            sends += 1;
            true
        }),
        1
    );
    assert_eq!(sends, 0);

    // At the retry tick the slot is rearmed without sending.
    assert_eq!(outbox::pump(20_000_000, true, |_| true), 1);

    // The next call resends the whole message.
    sends = 0;
    assert_eq!(
        outbox::pump(20_000_001, true, |_| {
            sends += 1;
            true
        }),
        1
    );
    assert_eq!(sends, 1);

    // Third attempt, then the retry timer gives up on the fourth tick.
    assert_eq!(outbox::pump(40_000_001, true, |_| true), 1);
    assert_eq!(outbox::pump(40_000_002, true, |_| true), 1);
    assert_eq!(outbox::pump(60_000_002, true, |_| true), 0);

    // An acknowledgement drops a waiting slot immediately.
    outbox::submit(0x300, two.clone(), true).unwrap();
    outbox::acknowledge(0x300);
    assert_eq!(outbox::pump(0, true, |_| true), 0);

    // Two slots fill, the third message is refused.
    outbox::submit(0x401, one.clone(), true).unwrap();
    outbox::submit(0x402, one.clone(), true).unwrap();
    assert!(outbox::submit(0x403, one, true).is_err());

    outbox::acknowledge(0x401);
    outbox::acknowledge(0x402);
    assert_eq!(outbox::pump(0, true, |_| true), 0);
}
