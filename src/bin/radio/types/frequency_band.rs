// Defines the LR1121 frequency ranges used by this firmware.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrequencyBand {
    SubGhz,
    SBand,
    Ism2_4Ghz,
}

impl FrequencyBand {
    // Returns the largest encoded protocol packet accepted by the selected modem profile.
    pub const fn maximum_packet_size(self) -> usize {
        match self {
            Self::SubGhz | Self::Ism2_4Ghz => 255,
            Self::SBand => 133,
        }
    }

    pub const fn for_frequency(frequency_hz: u32) -> Option<Self> {
        match frequency_hz {
            150_000_000..=960_000_000 => Some(Self::SubGhz),
            1_900_000_000..=2_100_000_000 => Some(Self::SBand),
            2_400_000_000..=2_500_000_000 => Some(Self::Ism2_4Ghz),
            _ => None,
        }
    }

    pub const fn contains(self, frequency_hz: u32) -> bool {
        // Keep one range table: band selection and validation cannot drift.
        match Self::for_frequency(frequency_hz) {
            Some(band) => band as u8 == self as u8,
            None => false,
        }
    }
}
