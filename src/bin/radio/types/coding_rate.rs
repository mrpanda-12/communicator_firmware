// Selects the LoRa coding rate used by the LR1121 modem.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodingRate {
    // Sets the forward-error-correction rate to 4/5.
    Cr45,
    // Sets the forward-error-correction rate to 4/6.
    Cr46,
    // Sets the forward-error-correction rate to 4/7.
    Cr47,
    // Sets the forward-error-correction rate to 4/8.
    Cr48,
}

impl CodingRate {
    // Encodes the selected coding rate for the LR1121 modulation command.
    pub const fn register_value(self) -> u8 {
        match self {
            Self::Cr45 => 0x01,
            Self::Cr46 => 0x02,
            Self::Cr47 => 0x03,
            Self::Cr48 => 0x04,
        }
    }
}
