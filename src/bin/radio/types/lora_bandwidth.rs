use super::FrequencyBand;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum LoRaBandwidth {
    Khz62_5 = 62_500,
    Khz125 = 125_000,
    Khz203 = 203_000,
    Khz250 = 250_000,
    Khz406 = 406_000,
    Khz500 = 500_000,
    Khz812 = 812_000,
}

impl LoRaBandwidth {
    pub const fn hz(self) -> u32 {
        self as u32
    }

    pub const fn lr1121_encoding(self, band: FrequencyBand) -> Option<u8> {
        match (band, self) {
            (FrequencyBand::SubGhz, Self::Khz62_5) => Some(0x03),
            (FrequencyBand::SubGhz, Self::Khz125) => Some(0x04),
            (FrequencyBand::SubGhz, Self::Khz250) => Some(0x05),
            (FrequencyBand::SubGhz, Self::Khz500) => Some(0x06),
            (FrequencyBand::Ism2_4Ghz, Self::Khz62_5) => Some(0x03),
            (FrequencyBand::Ism2_4Ghz, Self::Khz125) => Some(0x04),
            (FrequencyBand::Ism2_4Ghz, Self::Khz203) => Some(0x0D),
            (FrequencyBand::Ism2_4Ghz, Self::Khz250) => Some(0x05),
            (FrequencyBand::Ism2_4Ghz, Self::Khz406) => Some(0x0E),
            (FrequencyBand::Ism2_4Ghz, Self::Khz500) => Some(0x06),
            (FrequencyBand::Ism2_4Ghz, Self::Khz812) => Some(0x0F),
            (FrequencyBand::Sband, _) => None,
            (_, Self::Khz203 | Self::Khz406 | Self::Khz812) => None,
        }
    }
}

impl Default for LoRaBandwidth {
    fn default() -> Self {
        Self::Khz125
    }
}
