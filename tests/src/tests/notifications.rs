use super::*;

#[test]
fn notification_flow() {
    // Drain anything a previous case might have left behind.
    let receiver = notify::POPUP_QUEUE.receiver();
    while receiver.try_receive().is_ok() {}

    let before = notify::history_revision();
    notify::history_changed();
    assert_eq!(notify::history_revision(), before + 1);

    notify::message_received(
        ConversationId::Direct(id(1)),
        id(1),
        "hello",
        PopupType::Text,
    );
    let popup = receiver.try_receive().unwrap();
    assert_eq!(popup.popup_type, PopupType::Text);
    assert_eq!(popup.conversation, ConversationId::Direct(id(1)));
    assert_eq!(popup.sender_id, id(1));
    assert_eq!(popup.text.as_str(), "hello");
    assert!(popup.name.is_empty());
    assert_eq!(notify::history_revision(), before + 2);

    // A position toast keeps its own kind so the screen can tag it.
    notify::message_received(
        ConversationId::Direct(id(2)),
        id(2),
        "[GPS]",
        PopupType::Gps,
    );
    let popup = receiver.try_receive().unwrap();
    assert_eq!(popup.popup_type, PopupType::Gps);
    assert_eq!(popup.text.as_str(), "[GPS]");

    // Long text is cut to 31 characters plus an ellipsis.
    let long = "b".repeat(40);
    notify::message_received(ConversationId::Broadcast, id(2), &long, PopupType::Text);
    let popup = receiver.try_receive().unwrap();
    assert_eq!(popup.text.len(), 34);
    assert!(popup.text.ends_with("..."));
    assert_eq!(popup.text.chars().filter(|c| *c == 'b').count(), 31);

    notify::storage_full(ConversationId::Broadcast);
    let popup = receiver.try_receive().unwrap();
    assert_eq!(popup.popup_type, PopupType::Alert);
    assert_eq!(popup.text.as_str(), "STORAGE FULL");
    assert_eq!(popup.sender_id, [0; 16]);

    notify::send_queue_full(ConversationId::Direct(id(3)));
    let popup = receiver.try_receive().unwrap();
    assert_eq!(popup.popup_type, PopupType::Alert);
    assert_eq!(popup.text.as_str(), "SEND QUEUE FULL");
    assert_eq!(popup.conversation, ConversationId::Direct(id(3)));

    notify::contact_request(id(4));
    let popup = receiver.try_receive().unwrap();
    assert_eq!(popup.popup_type, PopupType::ContactRequest);
    assert_eq!(popup.text.as_str(), "WANTS TO ADD YOU");
    assert_eq!(popup.conversation, ConversationId::Direct(id(4)));
    assert_eq!(popup.sender_id, id(4));

    // A retune request carries the frequency the user can accept.
    notify::frequency_change_request(id(6), 869_400_000);
    let popup = receiver.try_receive().unwrap();
    assert_eq!(
        popup.popup_type,
        PopupType::FrequencyChangeRequest {
            frequency_hz: 869_400_000
        }
    );
    assert_eq!(popup.text.as_str(), "WANTS TO RETUNE");
    assert_eq!(popup.sender_id, id(6));

    // The queue holds eight; a ninth message drops the oldest.
    for index in 0..9u8 {
        let text = [
            "zero", "one", "two", "three", "four", "five", "six", "seven", "eight",
        ][index as usize];
        notify::message_received(ConversationId::Broadcast, id(5), text, PopupType::Text);
    }
    let first = receiver.try_receive().unwrap();
    assert_eq!(first.text.as_str(), "one", "oldest toast is dropped");
    let mut remaining = 0;
    while receiver.try_receive().is_ok() {
        remaining += 1;
    }
    assert_eq!(remaining, 7);
}
