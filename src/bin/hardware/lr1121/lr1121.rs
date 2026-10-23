use super::opcode::Opcode;
use crate::radio::{
    radio_error::RadioError,
    types::{
        LrFhssBitRate, LrFhssProfile, LrFhssShaping, LrFhssSyncWord, PacketStatus, RadioStatus,
        RadioVersion, RxBufferStatus,
    },
};
use embedded_hal::spi::SpiBus;
use esp_hal::delay::Delay;
use esp_hal::gpio::{Input, Output};
use esp_hal::spi::Error as SpiError;
use esp_hal::time::Instant;

const MAX_BUFFER_LENGTH: usize = u8::MAX as usize;
const MAX_TIMEOUT: u32 = 0x00FF_FFFF;
const BUSY_TIMEOUT_US: u64 = 500_000;
const IMAGE_CALIBRATION_STEP_MHZ: u32 = 4;

// Board RF switch setup from the Waveshare LR1121 reference code.
const BOARD_RF_SWITCH: RfSwitchConfig = RfSwitchConfig {
    enable: 0x03, // RFSW0 | RFSW1
    standby: 0x00,
    rx: 0x01, // RFSW0
    tx: 0x02, // RFSW1
    tx_hp: 0x02,
    tx_hf: 0x02,
    gnss: 0x00,
    wifi: 0x00,
};

const REG_MODE_DCDC: u8 = 0x01;
const TCXO_CTRL_3_0V: u8 = 0x06;
const TCXO_STARTUP_TIME: u32 = 300;
const LFCLK_XTAL: u8 = 0x01;

// RSSI calibration tables from lr1121_common.c.
const RSSI_CAL_BELOW_600MHZ: RssiCalibration = RssiCalibration {
    gain_offset: 0,
    gain_tune: [12, 12, 14, 0, 1, 3, 4, 4, 3, 6, 6, 6, 6, 6, 6, 6, 6],
};

const RSSI_CAL_600MHZ_TO_2GHZ: RssiCalibration = RssiCalibration {
    gain_offset: 0,
    gain_tune: [2, 2, 2, 3, 3, 4, 5, 4, 4, 6, 5, 5, 6, 6, 6, 7, 6],
};

const RSSI_CAL_ABOVE_2GHZ: RssiCalibration = RssiCalibration {
    gain_offset: 2030,
    gain_tune: [6, 7, 6, 4, 3, 4, 14, 12, 14, 12, 12, 12, 12, 8, 8, 9, 9],
};

struct RssiCalibration {
    gain_offset: i16,
    gain_tune: [u8; 17],
}

#[derive(Clone, Copy)]
struct RfSwitchConfig {
    enable: u8,
    standby: u8,
    rx: u8,
    tx: u8,
    tx_hp: u8,
    tx_hf: u8,
    gnss: u8,
    wifi: u8,
}

// Implements the LR1121 SPI driver.
pub struct Lr1121<'a, SPI> {
    spi: SPI,
    nss: Output<'a>,
    busy: Input<'a>,
    reset: Output<'a>,
    irq: Input<'a>,
}

impl<'a, SPI> Lr1121<'a, SPI>
where
    SPI: SpiBus<Error = SpiError>,
{
    pub fn new(
        spi: SPI,
        nss: Output<'a>,
        busy: Input<'a>,
        reset: Output<'a>,
        irq: Input<'a>,
    ) -> Result<Self, RadioError> {
        let mut device = Self {
            spi,
            nss,
            busy,
            reset,
            irq,
        };

        device.hard_reset()?;
        device.configure_system()?;
        Ok(device)
    }

    pub async fn wait_for_irq(&mut self) {
        let _ = self.irq.wait_for_high().await;
    }

    pub async fn wait_for_irq_low(&mut self) {
        let _ = self.irq.wait_for_low().await;
    }

    pub fn irq_is_high(&self) -> bool {
        self.irq.is_high()
    }

    // Resets the LR1121 and waits for BUSY to return low before startup continues.
    pub fn hard_reset(&mut self) -> Result<(), RadioError> {
        self.nss.set_high();
        self.reset.set_low();

        // The reference HAL holds reset low for 1 ms.
        Delay::new().delay_millis(1);

        self.reset.set_high();

        // Allows the chip to begin its startup sequence.
        Delay::new().delay_millis(2);

        self.wakeup()?;
        self.wait_on_busy()
    }

    // Pulls NSS low briefly so the chip leaves sleep mode, as the HAL does.
    pub fn wakeup(&mut self) -> Result<(), RadioError> {
        Delay::new().delay_millis(1);
        self.nss.set_low();
        Delay::new().delay_millis(1);
        self.nss.set_high();
        Delay::new().delay_millis(1);

        Ok(())
    }

    // Stops waiting and returns an error if the chip remains busy too long.
    fn wait_on_busy(&mut self) -> Result<(), RadioError> {
        let started = Instant::now();

        while self.busy.is_high() {
            if started.elapsed().as_micros() >= BUSY_TIMEOUT_US {
                return Err(RadioError::BusyTimeout);
            }
            core::hint::spin_loop();
        }

        Ok(())
    }

    // Asserts NSS for one transaction and releases it after the SPI bus idles.
    fn with_nss<T>(
        &mut self,
        transaction: impl FnOnce(&mut SPI) -> Result<T, SpiError>,
    ) -> Result<T, RadioError> {
        self.nss.set_low();

        let operation_result = transaction(&mut self.spi);
        let flush_result = self.spi.flush();

        self.nss.set_high();

        match operation_result {
            Err(error) => Err(RadioError::Spi(error)),
            Ok(value) => {
                flush_result?;
                Ok(value)
            }
        }
    }

    // Sends one opcode and its parameters in a single SPI transaction.
    fn write_command(&mut self, opcode: Opcode, data: &[u8]) -> Result<(), RadioError> {
        self.wait_on_busy()?;
        let opcode_bytes = opcode.to_bytes();

        self.with_nss(|spi| {
            spi.write(&opcode_bytes)?;
            if !data.is_empty() {
                spi.write(data)?;
            }
            Ok(())
        })
    }

    // Sends a command, waits for completion, and reads its response data.
    fn read_command_with_params(
        &mut self,
        opcode: Opcode,
        params: &[u8],
        target: &mut [u8],
    ) -> Result<(), RadioError> {
        self.write_command(opcode, params)?;
        self.wait_on_busy()?;

        self.with_nss(|spi| {
            let mut status = [0u8; 1];

            // Clocks out Stat1 first by sending a zero/NOP byte on MOSI.
            spi.transfer_in_place(&mut status)?;
            spi.flush()?;

            // Reads response data after Stat1 by sending zero bytes.
            if !target.is_empty() {
                target.fill(0);
                spi.transfer_in_place(target)?;
            }

            Ok(())
        })
    }

    // Reads a command response without request parameters.
    pub fn read_command(&mut self, opcode: Opcode, target: &mut [u8]) -> Result<(), RadioError> {
        self.read_command_with_params(opcode, &[], target)
    }

    // Reads Stat1, Stat2, and the 32-bit IRQ status in one transaction.
    // GetStatus is a direct read: the opcode must not be clocked out first.
    pub fn get_status(&mut self) -> Result<RadioStatus, RadioError> {
        self.wait_on_busy()?;

        let mut rx = [0u8; 6];

        self.with_nss(|spi| spi.transfer_in_place(&mut rx))?;

        Ok(RadioStatus {
            stat1: rx[0],
            stat2: rx[1],
            irq_status: u32::from_be_bytes([rx[2], rx[3], rx[4], rx[5]]),
        })
    }

    // Clears the reset status information by reading GetStatus without data.
    pub fn clear_reset_status(&mut self) -> Result<(), RadioError> {
        self.write_command(Opcode::GetStatus, &[])
    }

    // Returns the currently pending interrupt flags.
    pub fn get_irq_status(&mut self) -> Result<u32, RadioError> {
        Ok(self.get_status()?.irq_status)
    }

    // Reads hardware, use-case, and firmware version fields.
    pub fn get_version(&mut self) -> Result<RadioVersion, RadioError> {
        let mut data = [0u8; 4];
        self.read_command(Opcode::GetVersion, &mut data)?;

        Ok(RadioVersion {
            hw_version: data[0],
            use_case: data[1],
            fw_major: data[2],
            fw_minor: data[3],
        })
    }

    // Reads the device error flags.
    pub fn get_errors(&mut self) -> Result<u16, RadioError> {
        let mut data = [0u8; 2];
        self.read_command(Opcode::GetErrors, &mut data)?;
        Ok(u16::from_be_bytes(data))
    }

    // Clears the device error flags.
    pub fn clear_errors(&mut self) -> Result<(), RadioError> {
        self.write_command(Opcode::ClearErrors, &[])
    }

    // Calibrates the image-rejection path for the requested frequency range.
    // Freq1 and Freq2 are the LR1121's 4 MHz-step calibration indices.
    pub fn calibrate_image(&mut self, freq1: u8, freq2: u8) -> Result<(), RadioError> {
        if freq1 > freq2 {
            return Err(RadioError::InvalidParameter(
                "image calibration start must not exceed its end",
            ));
        }

        self.write_command(Opcode::CalibrateImage, &[freq1, freq2])
    }

    // Calibrates image rejection using MHz bounds, as the reference driver does.
    pub fn calibrate_image_in_mhz(
        &mut self,
        freq1_mhz: u32,
        freq2_mhz: u32,
    ) -> Result<(), RadioError> {
        if freq1_mhz > freq2_mhz {
            return Err(RadioError::InvalidParameter(
                "image calibration start must not exceed its end",
            ));
        }

        let freq1 = (freq1_mhz / IMAGE_CALIBRATION_STEP_MHZ) as u8;
        let freq2 = freq2_mhz.div_ceil(IMAGE_CALIBRATION_STEP_MHZ) as u8;

        self.calibrate_image(freq1, freq2)
    }

    // Runs the full system startup sequence used by the Waveshare examples.
    pub fn configure_system(&mut self) -> Result<(), RadioError> {
        self.enable_spi_crc(false)?;
        self.set_reg_mode(REG_MODE_DCDC)?;
        self.set_dio_as_rf_switch(BOARD_RF_SWITCH)?;

        // The TCXO must be configured before the chip enters XOSC standby.
        self.set_tcxo_mode(TCXO_CTRL_3_0V, TCXO_STARTUP_TIME)?;
        self.cfg_lfclk(LFCLK_XTAL, true)?;
        self.set_standby(1)?;

        self.clear_errors()?;
        self.calibrate(0x3F)?;
        self.clear_errors()?;
        self.clear_irq(u32::MAX)?;

        Ok(())
    }

    // Enables or disables CRC checks over the SPI link.
    pub fn enable_spi_crc(&mut self, enable: bool) -> Result<(), RadioError> {
        self.write_command(Opcode::EnableSpiCrc, &[u8::from(enable)])
    }

    // Selects the regulator mode (0 = LDO, 1 = DCDC).
    pub fn set_reg_mode(&mut self, mode: u8) -> Result<(), RadioError> {
        if mode > 1 {
            return Err(RadioError::InvalidParameter(
                "regulator mode must be 0 (LDO) or 1 (DCDC)",
            ));
        }

        self.write_command(Opcode::SetRegMode, &[mode])
    }

    // Routes the RF switch controls onto the chip's DIO pins.
    fn set_dio_as_rf_switch(&mut self, config: RfSwitchConfig) -> Result<(), RadioError> {
        self.write_command(
            Opcode::SetDioAsRfSwitch,
            &[
                config.enable,
                config.standby,
                config.rx,
                config.tx,
                config.tx_hp,
                config.tx_hf,
                config.gnss,
                config.wifi,
            ],
        )
    }

    // Configures the TCXO control voltage and startup time in 30.52 us steps.
    pub fn set_tcxo_mode(&mut self, voltage: u8, timeout: u32) -> Result<(), RadioError> {
        if timeout > MAX_TIMEOUT {
            return Err(RadioError::InvalidParameter(
                "TCXO timeout must fit in 24 bits",
            ));
        }

        let timeout = [
            ((timeout >> 16) & 0xFF) as u8,
            ((timeout >> 8) & 0xFF) as u8,
            (timeout & 0xFF) as u8,
        ];

        self.write_command(
            Opcode::SetTcxoMode,
            &[voltage, timeout[0], timeout[1], timeout[2]],
        )
    }

    // Selects the low-frequency clock and optionally waits for it to be ready.
    pub fn cfg_lfclk(&mut self, clock: u8, wait_ready: bool) -> Result<(), RadioError> {
        self.write_command(Opcode::CfgLfclk, &[clock | (u8::from(wait_ready) << 2)])
    }

    // Runs the requested calibration blocks (0x3F performs them all).
    pub fn calibrate(&mut self, calibration: u8) -> Result<(), RadioError> {
        self.write_command(Opcode::Calibrate, &[calibration])
    }

    // Enables or disables the RX boost mode.
    pub fn set_rx_boosted(&mut self, enable: bool) -> Result<(), RadioError> {
        self.write_command(Opcode::SetRxBoosted, &[u8::from(enable)])
    }

    // Loads the RSSI calibration table matching the operating frequency.
    pub fn set_rssi_calibration_for_frequency(
        &mut self,
        frequency_hz: u32,
    ) -> Result<(), RadioError> {
        let table = if frequency_hz < 600_000_000 {
            &RSSI_CAL_BELOW_600MHZ
        } else if frequency_hz <= 2_000_000_000 {
            &RSSI_CAL_600MHZ_TO_2GHZ
        } else {
            &RSSI_CAL_ABOVE_2GHZ
        };

        self.set_rssi_calibration(table)
    }

    // Packs and writes an RSSI calibration table exactly as the C driver does.
    fn set_rssi_calibration(&mut self, table: &RssiCalibration) -> Result<(), RadioError> {
        let t = &table.gain_tune;
        let gain_offset = table.gain_offset.to_be_bytes();
        let params = [
            (t[1] << 4) | t[0],
            (t[3] << 4) | t[2],
            (t[5] << 4) | t[4],
            (t[7] << 4) | t[6],
            (t[9] << 4) | t[8],
            (t[11] << 4) | t[10],
            (t[13] << 4) | t[12],
            (t[15] << 4) | t[14],
            t[16],
            gain_offset[0],
            gain_offset[1],
        ];

        self.write_command(Opcode::SetRssiCalibration, &params)
    }

    // Sets the LoRa PHY sync word used for this custom packet network.
    pub fn set_lora_sync_word(&mut self, sync_word: u8) -> Result<(), RadioError> {
        self.write_command(Opcode::SetLoraSyncWord, &[sync_word])
    }

    // Clears selected IRQ flags by writing ones to their bits.
    pub fn clear_irq(&mut self, irq_mask: u32) -> Result<(), RadioError> {
        self.write_command(Opcode::ClearIrq, &irq_mask.to_be_bytes())
    }

    // Enters standby with 0 for RC or 1 for the external oscillator.
    pub fn set_standby(&mut self, mode: u8) -> Result<(), RadioError> {
        if mode > 1 {
            return Err(RadioError::InvalidParameter(
                "standby mode must be 0 (RC) or 1 (XOSC)",
            ));
        }

        self.write_command(Opcode::SetStandby, &[mode])
    }

    // Selects the LoRa modem.
    pub fn set_lora_packet_type(&mut self) -> Result<(), RadioError> {
        self.set_packet_type(0x02)
    }

    // Selects the LR-FHSS transmit-only modem.
    pub fn set_lr_fhss_packet_type(&mut self) -> Result<(), RadioError> {
        self.set_packet_type(0x04)
    }

    // Selects a packet type supported by the LR1121.
    pub fn set_packet_type(&mut self, packet_type: u8) -> Result<(), RadioError> {
        self.write_command(Opcode::SetPacketType, &[packet_type])
    }

    // Reads the currently selected packet type.
    pub fn get_packet_type(&mut self) -> Result<u8, RadioError> {
        let mut data = [0u8; 1];
        self.read_command(Opcode::GetPacketType, &mut data)?;
        Ok(data[0])
    }

    // Reads the length and start offset of the last received packet.
    pub fn get_rx_buffer_status(&mut self) -> Result<RxBufferStatus, RadioError> {
        let mut data = [0u8; 2];
        self.read_command(Opcode::GetRxBufferStatus, &mut data)?;

        Ok(RxBufferStatus {
            payload_len: data[0],
            buffer_offset: data[1],
        })
    }

    // Reads RSSI in half-dB units and SNR in quarter-dB units.
    pub fn get_packet_status(&mut self) -> Result<PacketStatus, RadioError> {
        let mut data = [0u8; 3];
        self.read_command(Opcode::GetPacketStatus, &mut data)?;

        Ok(PacketStatus {
            rssi_packet_half_db: -i16::from(data[0]),
            snr_quarter_db: data[1] as i8,
            signal_rssi_half_db: -i16::from(data[2]),
        })
    }

    // Reads instantaneous RSSI in signed half-dB units.
    pub fn get_rssi_inst_half_db(&mut self) -> Result<i16, RadioError> {
        let mut data = [0u8; 1];
        self.read_command(Opcode::GetRssiInst, &mut data)?;
        Ok(-i16::from(data[0]))
    }

    // Encodes a command timeout as a 24-bit big-endian value.
    fn timeout_params(timeout: u32) -> Result<[u8; 3], RadioError> {
        if timeout > MAX_TIMEOUT {
            return Err(RadioError::InvalidParameter("timeout must fit in 24 bits"));
        }

        Ok([
            ((timeout >> 16) & 0xFF) as u8,
            ((timeout >> 8) & 0xFF) as u8,
            (timeout & 0xFF) as u8,
        ])
    }

    // Starts RX using 32.768 kHz RTC timeout ticks.
    // 0 receives one packet, 0xFF_FFFF keeps RX active continuously.
    pub fn set_rx(&mut self, timeout: u32) -> Result<(), RadioError> {
        let params = Self::timeout_params(timeout)?;
        self.write_command(Opcode::SetRx, &params)
    }

    // Starts TX and disables its timeout when set to zero.
    pub fn set_tx(&mut self, timeout: u32) -> Result<(), RadioError> {
        let params = Self::timeout_params(timeout)?;
        self.write_command(Opcode::SetTx, &params)
    }

    // Sets the RF frequency in Hz.
    pub fn set_rf_frequency(&mut self, frequency_hz: u32) -> Result<(), RadioError> {
        self.write_command(Opcode::SetRfFrequency, &frequency_hz.to_be_bytes())
    }

    // Configures the LR-FHSS modem bit rate and Gaussian shaping.
    pub fn set_lr_fhss_modulation_params(&mut self) -> Result<(), RadioError> {
        let bit_rate = (LrFhssBitRate::Default as u32).to_be_bytes();
        let params = [
            bit_rate[0],
            bit_rate[1],
            bit_rate[2],
            bit_rate[3],
            LrFhssShaping::GaussianBt1 as u8,
        ];
        self.write_command(Opcode::SetModulationParams, &params)
    }

    // Sets the default LR-FHSS sync word used by the matching modem profile.
    pub fn set_lr_fhss_sync_word(&mut self) -> Result<(), RadioError> {
        self.write_command(
            Opcode::LrFhssSetSyncWord,
            &(LrFhssSyncWord::Default as u32).to_be_bytes(),
        )
    }

    // Asks the LR1121 to build an LR-FHSS frame from an application payload.
    pub fn build_lr_fhss_frame(&mut self, payload: &[u8]) -> Result<(), RadioError> {
        let profile = LrFhssProfile::SbandDefault;
        let hop_sequence_seed = profile.hop_seed() as u16;
        let device_offset = profile.device_offset() as i8 as u8;
        let maximum_payload = profile.max_payload_size();

        if payload.is_empty() || payload.len() > maximum_payload {
            return Err(RadioError::PayloadTooLong {
                length: payload.len(),
                maximum: maximum_payload,
            });
        }

        let mut params = [0u8; 9 + LrFhssProfile::SbandDefault.max_payload_size()];
        params[0] = profile.header_count() as u8;
        params[1] = profile.coding_rate() as u8;
        params[2] = profile.modulation() as u8;
        params[3] = profile.grid_spacing() as u8;
        params[4] = profile.hopping() as u8;
        params[5] = profile.bandwidth() as u8;
        params[6..8].copy_from_slice(&hop_sequence_seed.to_be_bytes());
        params[8] = device_offset;
        params[9..9 + payload.len()].copy_from_slice(payload);

        // The reference frame builder programs the sync word before building.
        self.set_lr_fhss_sync_word()?;
        self.write_command(Opcode::LrFhssBuildFrame, &params[..9 + payload.len()])
    }

    // Configures LoRa spreading factor, bandwidth, coding rate, and LDRO.
    pub fn set_lora_modulation_params(
        &mut self,
        spreading_factor: u8,
        bandwidth_encoding: u8,
        coding_rate_encoding: u8,
        low_data_rate_optimization: bool,
    ) -> Result<(), RadioError> {
        if !(5..=12).contains(&spreading_factor) {
            return Err(RadioError::InvalidParameter(
                "LoRa spreading factor must be 5 through 12",
            ));
        }

        let valid_bandwidth = (0x03..=0x06).contains(&bandwidth_encoding)
            || (0x0D..=0x0F).contains(&bandwidth_encoding);
        if !valid_bandwidth {
            return Err(RadioError::InvalidParameter(
                "unsupported LR1121 LoRa bandwidth encoding",
            ));
        }

        if !(1..=7).contains(&coding_rate_encoding) {
            return Err(RadioError::InvalidParameter(
                "LoRa coding-rate encoding must be 1 through 7",
            ));
        }

        let params = [
            spreading_factor,
            bandwidth_encoding,
            coding_rate_encoding,
            u8::from(low_data_rate_optimization),
        ];

        self.write_command(Opcode::SetModulationParams, &params)
    }

    // Configures the LoRa packet format and payload length handling.
    pub fn set_lora_packet_params(
        &mut self,
        preamble_symbols: u16,
        implicit_header: bool,
        payload_len: u8,
        crc_enabled: bool,
        invert_iq: bool,
    ) -> Result<(), RadioError> {
        if preamble_symbols == 0 {
            return Err(RadioError::InvalidParameter(
                "LoRa preamble length must be at least one symbol",
            ));
        }

        let preamble = preamble_symbols.to_be_bytes();
        let params = [
            preamble[0],
            preamble[1],
            u8::from(implicit_header),
            payload_len,
            u8::from(crc_enabled),
            u8::from(invert_iq),
        ];

        self.write_command(Opcode::SetPacketParams, &params)
    }

    // Configures PA selection, supply, duty cycle, and high-power settings.
    pub fn set_pa_config(
        &mut self,
        pa_select: u8,
        regulator_supply: u8,
        duty_cycle: u8,
        high_power_pa_select: u8,
    ) -> Result<(), RadioError> {
        if pa_select > 2 {
            return Err(RadioError::InvalidParameter(
                "PA selection must be 0 (LP), 1 (HP), or 2 (HF)",
            ));
        }
        if regulator_supply > 1 {
            return Err(RadioError::InvalidParameter(
                "PA supply must be 0 (regulator) or 1 (VBAT)",
            ));
        }
        if duty_cycle > 7 || high_power_pa_select > 7 {
            return Err(RadioError::InvalidParameter(
                "PA duty-cycle and HP selection values must be 0 through 7",
            ));
        }

        let params = [
            pa_select,
            regulator_supply,
            duty_cycle,
            high_power_pa_select,
        ];

        self.write_command(Opcode::SetPaConfig, &params)
    }

    // Sets TX power in dBm and the PA ramp time.
    pub fn set_tx_params(&mut self, power_dbm: i8, ramp_time: u8) -> Result<(), RadioError> {
        self.write_command(Opcode::SetTxParams, &[power_dbm as u8, ramp_time])
    }

    // Selects the post-TX/RX mode: 1 for RC standby, 2 for XOSC standby, or 3 for FS.
    pub fn set_rx_tx_fallback_mode(&mut self, mode: u8) -> Result<(), RadioError> {
        if !(1..=3).contains(&mode) {
            return Err(RadioError::InvalidParameter(
                "fallback mode must be 1 (RC), 2 (XOSC), or 3 (FS)",
            ));
        }

        self.write_command(Opcode::SetRxTxFallbackMode, &[mode])
    }

    // Writes payload bytes to the transmit buffer.
    pub fn write_buffer(&mut self, payload: &[u8]) -> Result<(), RadioError> {
        if payload.len() > MAX_BUFFER_LENGTH {
            return Err(RadioError::PayloadTooLong {
                length: payload.len(),
                maximum: MAX_BUFFER_LENGTH,
            });
        }

        self.write_command(Opcode::WriteBuffer, payload)
    }

    // Reads bytes from the receive buffer at the requested offset.
    pub fn read_buffer(&mut self, offset: u8, target: &mut [u8]) -> Result<(), RadioError> {
        if target.len() > MAX_BUFFER_LENGTH {
            return Err(RadioError::PayloadTooLong {
                length: target.len(),
                maximum: MAX_BUFFER_LENGTH,
            });
        }

        if target.is_empty() {
            return Ok(());
        }

        let params = [offset, target.len() as u8];
        self.read_command_with_params(Opcode::ReadBuffer, &params, target)
    }

    // Clears the receive buffer.
    pub fn clear_rx_buffer(&mut self) -> Result<(), RadioError> {
        self.write_command(Opcode::ClearRxBuffer, &[])
    }

    // Routes selected IRQ flags to the two IRQ output pins.
    pub fn set_dio_irq_params(&mut self, irq1_mask: u32, irq2_mask: u32) -> Result<(), RadioError> {
        let mut params = [0u8; 8];
        params[..4].copy_from_slice(&irq1_mask.to_be_bytes());
        params[4..].copy_from_slice(&irq2_mask.to_be_bytes());
        self.write_command(Opcode::SetDioIrqParams, &params)
    }
}
