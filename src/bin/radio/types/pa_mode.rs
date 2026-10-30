// Selects the LR1121 transmit power-amplifier path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaMode {
    // Uses the lower-power PA path for shorter-range links.
    LowPower,
    // Uses the high-power PA path for longer-range links within board limits.
    HighPower,
    // Uses the high-frequency PA path for S-band and 2.4 GHz operation.
    HighFrequencyPath,
}

impl PaMode {
    // Validates the requested TX power against the selected PA mode limit.
    pub(crate) const fn maximum_tx_power_dbm(self) -> i8 {
        match self {
            Self::LowPower => 14,
            Self::HighPower => 22,
            Self::HighFrequencyPath => 13,
        }
    }

    // Encodes the PA selection, supply, duty cycle, and high-power setup values.
    // This board wires sub-GHz output through the HP PA for every power level,
    // so both sub-GHz modes use the reference HP configuration.
    pub(crate) const fn lr1121_register_values(self) -> (u8, u8, u8, u8) {
        match self {
            Self::LowPower | Self::HighPower => (0x01, 0x01, 0x04, 0x07),
            Self::HighFrequencyPath => (0x02, 0x00, 0x07, 0x00),
        }
    }
}
