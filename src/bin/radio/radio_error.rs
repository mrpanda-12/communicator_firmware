use crate::packet::packet_error::PacketError;
use esp_hal::spi::Error;

#[derive(Debug)]
pub enum RadioError {
    Spi(Error),
    Packet(PacketError),
    BusyTimeout,
    InvalidParameter(&'static str),
    PayloadTooLong { length: usize, maximum: usize },
    InvalidConfiguration,
    ImplicitPayloadLengthRequired,
    TxQueueFull,
    TxTimeout,
    RadioBusy,
}
impl From<Error> for RadioError {
    fn from(error: Error) -> Self {
        Self::Spi(error)
    }
}
