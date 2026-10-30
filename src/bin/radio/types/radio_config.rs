use crate::radio::types::{CodingRate, FrequencyBand, LoraBandwidth, LrFhssProfile, PaMode};

// Configures the LR1121 RF band, transmit path, and modem settings.
// LoRa-specific fields apply to sub-GHz and 2.4 GHz; S-band uses its LR-FHSS profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RadioConfig {
    // Sets the RF center frequency in Hz.
    // Defaults to 869_500_000 Hz for this project.
    pub frequency_hz: u32,

    // Identifies the supported LR1121 RF path for this frequency.
    pub frequency_band: FrequencyBand,

    // Sets the LoRa spreading factor.
    // Increases sensitivity while increasing airtime as the value rises.
    pub spreading_factor: u8,

    // Selects the LoRa network sync word for sub-GHz and 2.4 GHz LoRa.
    // Defaults to the private-network value 0x12; LR-FHSS uses its profile sync word.
    pub lora_sync_word: u8,

    // Sets the LoRa bandwidth when the selected band uses LoRa.
    pub bandwidth: LoraBandwidth,

    // Selects the LoRa forward-error-correction coding rate.
    pub coding_rate: CodingRate,

    // Sets the conducted transmit power in dBm.
    // Keeps the value within the selected PA mode's valid range.
    pub tx_power_dbm: i8,

    // Selects the LR1121 power-amplifier path.
    pub pa_mode: PaMode,

    // Sets the LoRa preamble length in symbols.
    // Requires matching settings on both communicating radios.
    pub preamble_symbols: u16,

    // Enables the LoRa PHY packet CRC to detect corrupted packets.
    pub crc_enabled: bool,

    // Enables the implicit fixed-length packet header.
    pub implicit_header: bool,

    // Enables inverted IQ modulation when interoperability requires it.
    pub invert_iq: bool,

    // Overrides the automatic low-data-rate optimization decision.
    pub low_data_rate_optimization: Option<bool>,
}

// Reports validation errors found in a radio configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigError {
    // Rejects a spreading factor outside the supported range.
    InvalidSpreadingFactor,

    // Rejects a bandwidth that the driver does not support.
    UnsupportedBandwidth,

    // Rejects a preamble length of zero.
    InvalidPreambleLength,

    // Rejects a transmit power outside the configured PA range.
    InvalidTransmitPower,

    // Rejects a frequency outside the driver-supported band.
    UnsupportedFrequency,

    // Rejects a PA path that does not match the selected RF band.
    UnsupportedPaMode,
}

impl RadioConfig {
    // Initializes a configuration for the supplied RF center frequency.
    // Leaves the remaining fields at the default project values.
    pub const fn new(frequency_hz: u32) -> Self {
        let frequency_band = match FrequencyBand::for_frequency(frequency_hz) {
            Some(band) => band,
            None => FrequencyBand::SubGhz,
        };
        Self {
            frequency_hz,
            frequency_band,
            spreading_factor: 9,
            lora_sync_word: 0x12,
            bandwidth: LoraBandwidth::Khz125,
            coding_rate: CodingRate::Cr45,
            tx_power_dbm: 10,
            pa_mode: match frequency_band {
                FrequencyBand::SubGhz => PaMode::LowPower,
                FrequencyBand::SBand | FrequencyBand::Ism2_4Ghz => PaMode::HighFrequencyPath,
            },
            preamble_symbols: 8,
            crc_enabled: true,
            implicit_header: false,
            invert_iq: false,
            low_data_rate_optimization: None,
        }
    }

    // Validates the basic radio settings before programming the modem.
    // Keeps board-specific and regional limits in the application layer.
    pub fn validate(&self) -> Result<(), ConfigError> {
        if !(5..=12).contains(&self.spreading_factor) {
            return Err(ConfigError::InvalidSpreadingFactor);
        }

        let bandwidth_supported = self
            .bandwidth
            .lr1121_encoding(self.frequency_band)
            .is_some()
            || self.frequency_band == FrequencyBand::SBand;
        if !bandwidth_supported {
            return Err(ConfigError::UnsupportedBandwidth);
        }

        if self.preamble_symbols == 0 {
            return Err(ConfigError::InvalidPreambleLength);
        }

        let pa_matches_band = match self.frequency_band {
            FrequencyBand::SubGhz => matches!(self.pa_mode, PaMode::LowPower | PaMode::HighPower),
            FrequencyBand::SBand | FrequencyBand::Ism2_4Ghz => {
                self.pa_mode == PaMode::HighFrequencyPath
            }
        };
        if !pa_matches_band {
            return Err(ConfigError::UnsupportedPaMode);
        }

        if !(-9..=self.pa_mode.maximum_tx_power_dbm()).contains(&self.tx_power_dbm) {
            return Err(ConfigError::InvalidTransmitPower);
        }

        if !self.frequency_band.contains(self.frequency_hz) {
            return Err(ConfigError::UnsupportedFrequency);
        }

        Ok(())
    }

    // Recommends the low-data-rate optimization setting for this modem config.
    // Enables LDRO when the symbol duration is long enough to benefit from it.
    pub fn recommended_ldro(&self) -> bool {
        let sf = u32::from(self.spreading_factor);
        let symbol_time_us = (1_u64 << sf) * 1_000_000 / u64::from(self.bandwidth.hz());

        symbol_time_us >= 16_000
    }

    // Returns the configured LDRO value or falls back to the recommendation.
    pub fn low_data_rate_optimization_enabled(&self) -> bool {
        self.low_data_rate_optimization
            .unwrap_or_else(|| self.recommended_ldro())
    }

    // Estimates how long one encoded frame keeps the transmitter busy.
    // Uses the Semtech LoRa time-on-air formula and an LR-FHSS bit-rate estimate;
    // the duty-cycle limiter consumes this value, so an overestimate is safe.
    pub fn time_on_air_us(&self, packet_len: usize) -> u64 {
        if self.frequency_band == FrequencyBand::SBand {
            let bandwidth = u64::from(LrFhssProfile::SBandDefault.bandwidth().hz());
            // LR-FHSS carries roughly three bits per seven hertz of channel bandwidth.
            let bit_rate = (bandwidth * 3 / 7).max(1);
            let bits = 8 * (packet_len as u64 + 24);
            return bits * 1_000_000 / bit_rate;
        }

        let spreading_factor = u64::from(self.spreading_factor);
        let bandwidth = u64::from(self.bandwidth.hz());
        let symbol_us = ((1u64 << spreading_factor) * 1_000_000).div_ceil(bandwidth);
        let coding_rate = u64::from(self.coding_rate.register_value());
        let ldro = u64::from(self.low_data_rate_optimization_enabled());
        let crc = u64::from(self.crc_enabled);
        let implicit_header = u64::from(self.implicit_header);

        let payload_bits = 8 * packet_len as u64;
        let adjusted = (payload_bits + 28 + 16 * crc)
            .saturating_sub(4 * spreading_factor)
            .saturating_sub(20 * implicit_header);
        let blocks = adjusted.div_ceil(4 * (spreading_factor - 2 * ldro));
        let payload_symbols = 8 + blocks * (coding_rate + 4);

        // The preamble ends with a 4.25-symbol sync, counted in quarter symbols.
        let preamble_quarters = 4 * u64::from(self.preamble_symbols) + 17;
        let total_quarters = preamble_quarters + 4 * payload_symbols;
        total_quarters * symbol_us / 4
    }
}

impl Default for RadioConfig {
    // Initializes the default 869.5 MHz sub-GHz LoRa configuration.
    fn default() -> Self {
        Self::new(869_500_000)
    }
}
