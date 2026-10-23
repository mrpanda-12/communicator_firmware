#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PacketError {
    PayloadTooLarge,
    InvalidFragmentMetadata,
    InvalidEncryptedPayload,
    UnsupportedVersion,
    InvalidPacketType,
    InvalidEncryptionFlag,
    PacketTooShort,
    InvalidAuthenticationTag,
    BufferTooSmall,
    InvalidHopLimit,
}
