use super::*;

#[test]
fn contact_labels_for_known_and_unknown_peers() {
    let broadcast = ContactEntry::broadcast();
    assert_eq!(broadcast.list_label().as_str(), "BROADCAST [PUBLIC]");
    assert!(!broadcast.is_deletable());
    assert!(!broadcast.is_unknown());

    let paired = ContactEntry::paired(id(0x12), "Alice", [1; KEY_SIZE], false);
    assert_eq!(paired.list_label().as_str(), "Alice");
    assert_eq!(paired.name_or_id().as_str(), "Alice");
    assert!(!paired.is_unknown());

    let unnamed = ContactEntry::paired(id(0x12), "", [1; KEY_SIZE], false);
    assert!(unnamed.list_label().starts_with("PAIRED "));
    assert!(unnamed.list_label().ends_with("..."));
    assert_eq!(unnamed.name_or_id(), hex_device_id(&id(0x12)));

    let unknown = ContactEntry::unknown(id(0x34));
    assert!(unknown.list_label().starts_with("UNKNOWN "));
    assert!(unknown.is_unknown());
    assert_eq!(unknown.name_or_id(), hex_device_id(&id(0x34)));
}

#[test]
fn contact_book_adopts_and_updates() {
    let mut book = ContactBook::new();
    assert_eq!(book.entries().len(), 1);

    assert_eq!(book.adopt(&contact(0x11, "a", 1, false)), Ok(()));
    assert_eq!(book.adopt(&contact(0x22, "b", 2, false)), Ok(()));
    assert_eq!(book.entries().len(), 3);
    assert!(book.direct_key(&id(0x11)).is_some());
    assert!(book.entry_by_conversation(ConversationId::Direct(id(0x22))).is_some());

    // Adopting the same peer again updates in place.
    assert_eq!(book.adopt(&contact(0x11, "renamed", 9, true)), Ok(()));
    assert_eq!(book.entries().len(), 3);
    let entry = book
        .entry_by_conversation(ConversationId::Direct(id(0x11)))
        .unwrap();
    assert_eq!(entry.custom_name.as_str(), "renamed");
    assert!(entry.blocked);

    let reserved = Contact {
        device_id: [0; 16],
        key: [0; KEY_SIZE],
        custom_name: String::new(),
        blocked: false,
    };
    assert_eq!(
        book.adopt(&reserved),
        Err(crate::ui::contact::ContactBookError::ReservedBroadcastId)
    );
}

#[test]
fn contact_book_renames_and_registers_peers() {
    let mut book = ContactBook::new();
    book.adopt(&contact(0x11, "old", 1, false)).unwrap();

    book.set_name(id(0x11), "new");
    assert_eq!(
        book.entry_by_conversation(ConversationId::Direct(id(0x11)))
            .unwrap()
            .custom_name
            .as_str(),
        "new"
    );

    assert!(book.note_unknown(id(0x77)));
    assert!(!book.note_unknown(id(0x77)), "second note must be ignored");
    assert_eq!(
        book.entry_by_conversation(ConversationId::Direct(id(0x77)))
            .unwrap()
            .list_label()
            .as_str(),
        "UNKNOWN 7777..."
    );
}

#[test]
fn contact_book_blocks_and_removes() {
    let mut book = ContactBook::new();
    book.adopt(&contact(0x11, "a", 1, false)).unwrap();
    book.adopt(&contact(0x22, "b", 2, false)).unwrap();

    assert_eq!(book.visible_indices(false).len(), 3);
    assert_eq!(book.visible_indices(true).len(), 2);

    assert!(book.set_blocked(&id(0x11), true));
    assert_eq!(book.visible_indices(false).len(), 2);
    assert!(!book.set_blocked(&id(0x99), true), "unknown peer cannot be blocked");

    assert_eq!(
        book.remove(0),
        Err(crate::ui::contact::ContactBookError::BroadcastCannotBeDeleted)
    );
    assert_eq!(
        book.remove(99),
        Err(crate::ui::contact::ContactBookError::InvalidIndex)
    );
    assert!(book.remove(1).is_ok());
    assert!(book.entry_by_conversation(ConversationId::Direct(id(0x11))).is_none());
}

#[test]
fn contact_book_sorts_chats_by_activity() {
    let mut book = ContactBook::new();
    book.adopt(&contact(0x11, "a", 1, false)).unwrap();
    book.adopt(&contact(0x22, "b", 2, false)).unwrap();

    book.seed_preview(ConversationId::Direct(id(0x11)), "one", true, false, 100);
    book.seed_preview(ConversationId::Direct(id(0x22)), "two", false, false, 200);
    assert_eq!(book.chat_indices().as_slice(), &[2, 1, 0]);

    book.seed_preview(ConversationId::Direct(id(0x22)), "older", false, false, 50);
    assert_eq!(book.chat_indices().as_slice(), &[1, 2, 0]);

    // An unknown conversation is ignored rather than inserted.
    book.seed_preview(ConversationId::Direct(id(0x99)), "ignored", false, false, 999);
    assert_eq!(book.chat_indices().as_slice(), &[1, 2, 0]);
}

#[test]
fn preview_text_truncates_and_marks_gps() {
    assert_eq!(preview_text("short", None).as_str(), "short");

    let long = "a".repeat(40);
    let preview = preview_text(&long, None);
    assert_eq!(preview.len(), 22);
    assert!(preview.ends_with(".."));
    assert_eq!(preview.chars().filter(|c| *c == 'a').count(), 20);

    assert_eq!(
        preview_text("ignored", Some((1, 2))).as_str(),
        "[GPS]"
    );
}
