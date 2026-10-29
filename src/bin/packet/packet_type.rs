#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum PacketType {
    Text = 0x01,
    Ack = 0x02,
    Coordinates = 0x03,
    Heading = 0x04,
    DeviceAnnounce = 0x05,
    FrequencyChange = 0x06,
    DeviceAnnounceResponse = 0x07,
    PairingConfirm = 0x08,
}

impl TryFrom<u8> for PacketType {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x01 => Ok(Self::Text),
            0x02 => Ok(Self::Ack),
            0x03 => Ok(Self::Coordinates),
            0x04 => Ok(Self::Heading),
            0x05 => Ok(Self::DeviceAnnounce),
            0x06 => Ok(Self::FrequencyChange),
            0x07 => Ok(Self::DeviceAnnounceResponse),
            0x08 => Ok(Self::PairingConfirm),
            _ => Err(()),
        }
    }
}
