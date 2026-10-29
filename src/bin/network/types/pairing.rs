use crate::crypto::{self, KEY_SIZE};

/// One handshake in progress with a peer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PendingPairing {
    pub peer_id: [u8; 16],
    pub key: [u8; KEY_SIZE],
    pub request_nonce: Option<[u8; crypto::NONCE_SIZE]>,
    pub response_nonce: Option<[u8; crypto::NONCE_SIZE]>,
}

/// Outcome of one handshake, delivered to the UI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PairingEvent {
    pub peer_id: [u8; 16],
    pub succeeded: bool,
    pub key: [u8; KEY_SIZE],
}

/// Stage encoded in the first payload byte of a confirmation packet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PairingStage {
    Request = 0,
    Challenge = 1,
    Finish = 2,
}
