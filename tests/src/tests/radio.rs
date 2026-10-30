use super::*;

#[test]
fn frequency_bands_cover_expected_ranges() {
    assert_eq!(
        FrequencyBand::for_frequency(869_400_000),
        Some(FrequencyBand::SubGhz)
    );
    assert_eq!(
        FrequencyBand::for_frequency(150_000_000),
        Some(FrequencyBand::SubGhz)
    );
    assert_eq!(
        FrequencyBand::for_frequency(2_000_000_000),
        Some(FrequencyBand::SBand)
    );
    assert_eq!(
        FrequencyBand::for_frequency(2_400_000_000),
        Some(FrequencyBand::Ism2_4Ghz)
    );
    assert_eq!(FrequencyBand::for_frequency(100_000_000), None);
    assert_eq!(FrequencyBand::for_frequency(1_000_000_000), None);

    assert!(FrequencyBand::SubGhz.contains(869_400_000));
    assert!(!FrequencyBand::SubGhz.contains(2_400_000_000));
    assert!(FrequencyBand::SBand.contains(2_000_000_000));
}

#[test]
fn band_packet_sizes() {
    assert_eq!(FrequencyBand::SubGhz.maximum_packet_size(), 255);
    assert_eq!(FrequencyBand::Ism2_4Ghz.maximum_packet_size(), 255);
    assert_eq!(FrequencyBand::SBand.maximum_packet_size(), 133);
}

#[test]
fn lora_bandwidth_encodings() {
    assert_eq!(LoraBandwidth::Khz62_5.hz(), 62_500);
    assert_eq!(LoraBandwidth::Khz125.hz(), 125_000);
    assert_eq!(LoraBandwidth::Khz812.hz(), 812_000);

    assert_eq!(
        LoraBandwidth::Khz125.lr1121_encoding(FrequencyBand::SubGhz),
        Some(0x04)
    );
    assert_eq!(
        LoraBandwidth::Khz203.lr1121_encoding(FrequencyBand::SubGhz),
        None
    );
    assert_eq!(
        LoraBandwidth::Khz203.lr1121_encoding(FrequencyBand::Ism2_4Ghz),
        Some(0x0D)
    );
    assert_eq!(
        LoraBandwidth::Khz812.lr1121_encoding(FrequencyBand::Ism2_4Ghz),
        Some(0x0F)
    );
    assert_eq!(
        LoraBandwidth::Khz125.lr1121_encoding(FrequencyBand::SBand),
        None
    );
}

#[test]
fn coding_rates_encode_in_order() {
    assert_eq!(CodingRate::Cr45.register_value(), 0x01);
    assert_eq!(CodingRate::Cr46.register_value(), 0x02);
    assert_eq!(CodingRate::Cr47.register_value(), 0x03);
    assert_eq!(CodingRate::Cr48.register_value(), 0x04);
}

#[test]
fn pa_modes_cap_power() {
    assert_eq!(PaMode::LowPower.maximum_tx_power_dbm(), 14);
    assert_eq!(PaMode::HighPower.maximum_tx_power_dbm(), 22);
    assert_eq!(PaMode::HighFrequencyPath.maximum_tx_power_dbm(), 13);
    assert_eq!(
        PaMode::LowPower.lr1121_register_values(),
        PaMode::HighPower.lr1121_register_values()
    );
    assert_ne!(
        PaMode::HighPower.lr1121_register_values(),
        PaMode::HighFrequencyPath.lr1121_register_values()
    );
}

#[test]
fn radio_config_defaults_per_band() {
    let sub = RadioConfig::new(869_400_000);
    assert_eq!(sub.frequency_band, FrequencyBand::SubGhz);
    assert_eq!(sub.pa_mode, PaMode::LowPower);
    assert_eq!(sub.spreading_factor, 9);
    assert_eq!(sub.bandwidth, LoraBandwidth::Khz125);
    assert_eq!(sub.coding_rate, CodingRate::Cr45);
    assert_eq!(sub.preamble_symbols, 8);
    assert!(sub.crc_enabled);
    assert!(!sub.implicit_header);
    assert!(!sub.invert_iq);
    assert_eq!(sub.low_data_rate_optimization, None);
    assert_eq!(sub.validate(), Ok(()));

    assert_eq!(
        RadioConfig::new(2_400_000_000).pa_mode,
        PaMode::HighFrequencyPath
    );
    assert_eq!(RadioConfig::new(2_000_000_000).frequency_band, FrequencyBand::SBand);
    assert_eq!(RadioConfig::default().frequency_hz, 869_500_000);
}

#[test]
fn radio_config_validation_rejects_bad_values() {
    let mut config = RadioConfig::new(869_400_000);

    config.spreading_factor = 4;
    assert_eq!(
        config.validate().unwrap_err(),
        ConfigError::InvalidSpreadingFactor
    );
    config.spreading_factor = 13;
    assert_eq!(
        config.validate().unwrap_err(),
        ConfigError::InvalidSpreadingFactor
    );
    config.spreading_factor = 9;

    config.bandwidth = LoraBandwidth::Khz203;
    assert_eq!(
        config.validate().unwrap_err(),
        ConfigError::UnsupportedBandwidth
    );
    config.bandwidth = LoraBandwidth::Khz125;

    config.preamble_symbols = 0;
    assert_eq!(
        config.validate().unwrap_err(),
        ConfigError::InvalidPreambleLength
    );
    config.preamble_symbols = 8;

    config.tx_power_dbm = 30;
    assert_eq!(
        config.validate().unwrap_err(),
        ConfigError::InvalidTransmitPower
    );
    config.tx_power_dbm = 10;

    config.pa_mode = PaMode::HighFrequencyPath;
    assert_eq!(config.validate().unwrap_err(), ConfigError::UnsupportedPaMode);
    config.pa_mode = PaMode::LowPower;

    config.frequency_hz = 2_400_000_000;
    assert_eq!(
        config.validate().unwrap_err(),
        ConfigError::UnsupportedFrequency
    );
}

#[test]
fn ldro_recommendation_depends_on_symbol_time() {
    let mut config = RadioConfig::new(869_400_000);
    config.spreading_factor = 9;
    config.bandwidth = LoraBandwidth::Khz125;
    assert!(!config.recommended_ldro());
    assert!(!config.low_data_rate_optimization_enabled());

    config.spreading_factor = 12;
    assert!(config.recommended_ldro());
    assert!(config.low_data_rate_optimization_enabled());

    config.low_data_rate_optimization = Some(false);
    assert!(!config.low_data_rate_optimization_enabled());
}

#[test]
fn time_on_air_grows_with_spreading_factor() {
    let mut config = RadioConfig::new(869_400_000);
    config.spreading_factor = 7;
    let fast = config.time_on_air_us(50);
    config.spreading_factor = 12;
    let slow = config.time_on_air_us(50);
    assert!(fast > 0);
    assert!(slow > fast, "slow={slow} fast={fast}");

    // S-band takes the LR-FHSS estimate rather than the LoRa formula.
    let sband = RadioConfig::new(2_000_000_000);
    assert!(sband.time_on_air_us(50) > 0);
}

#[test]
fn lr_fhss_profile_values() {
    let profile = LrFhssProfile::SBandDefault;
    assert_eq!(profile.bandwidth().hz(), 136_720);
    assert_eq!(profile.max_payload_size(), 133);
    assert_eq!(
        profile.header_count(),
        crate::radio::types::LrFhssHeaderCount::Two
    );
    assert_eq!(
        profile.coding_rate(),
        crate::radio::types::LrFhssCodingRate::Cr5_6
    );
    assert_eq!(
        profile.hopping(),
        crate::radio::types::LrFhssHopping::Enabled
    );
}
