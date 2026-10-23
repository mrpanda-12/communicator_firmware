// Stores the length and offset of the most recently received packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RxBufferStatus {
    pub payload_len: u8,
    pub buffer_offset: u8,
}
