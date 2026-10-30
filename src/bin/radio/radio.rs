use core::ops::AsyncFnMut;

use embassy_futures::select::{Either, select};
use embassy_time::{Duration, Timer};
use embedded_hal::spi::SpiBus;
use esp_hal::spi::Error as SpiError;

use crate::hardware::lr1121::lr1121::Lr1121;
use crate::packet::packet_data::{MAX_PACKET_SIZE, PacketData};
use crate::radio::control::RadioControl;
use crate::radio::duty_cycle::DutyCycle;
use crate::radio::queue::{Packet, QueueError, RadioQueue};
use crate::radio::radio_error::RadioError;
use crate::radio::types::{FrequencyBand, LoraBandwidth, PaMode, PacketStatus, RadioConfig};

// Routes these LR1121 IRQ register bits to DIO9.
const IRQ_TX_DONE: u32 = 1 << 2;
const IRQ_RX_DONE: u32 = 1 << 3;
const IRQ_HEADER_ERROR: u32 = 1 << 6;
const IRQ_CRC_ERROR: u32 = 1 << 7;
const IRQ_TIMEOUT: u32 = 1 << 10;

const IRQ_DIO9_MASK: u32 =
    IRQ_TX_DONE | IRQ_RX_DONE | IRQ_HEADER_ERROR | IRQ_CRC_ERROR | IRQ_TIMEOUT;

// Sets a 30 s TX timeout.
const TX_TIMEOUT_TICKS: u32 = 30 * 32_768;

// Leaves a short RX window after this many queued transmissions.
const TX_BURST_LIMIT: usize = 2;
const RX_WINDOW: Duration = Duration::from_millis(500);

#[derive(Clone, Copy)]
#[repr(u8)]
enum TxRampTime {
    Us48 = 0x02,
}

#[derive(Clone, Copy)]
#[repr(u8)]
enum RxTxFallbackMode {
    StandbyRc = 0x01,
}

// Limits TX payloads to the packet buffer and the radio's one-byte length.
const MAX_TX_PAYLOAD: usize = if MAX_PACKET_SIZE < u8::MAX as usize {
    MAX_PACKET_SIZE
} else {
    u8::MAX as usize
};

pub struct Radio<'a, 'q, SPI> {
    pub lr1121: Lr1121<'a, SPI>,
    queue: &'q RadioQueue,
    control: &'q RadioControl,
    config: RadioConfig,
    tx_in_flight: bool,
    tx_burst_count: usize,
    rx_active: bool,
    rx_payload_len: Option<u8>,
    duty_cycle: DutyCycle,
}

impl<'a, 'q, SPI> Radio<'a, 'q, SPI>
where
    SPI: SpiBus<Error = SpiError>,
{
    pub fn new(
        config: RadioConfig,
        lr1121: Lr1121<'a, SPI>,
        queue: &'q RadioQueue,
        control: &'q RadioControl,
    ) -> Result<Self, RadioError> {
        config
            .validate()
            .map_err(|_| RadioError::InvalidConfiguration)?;

        let mut radio = Self {
            lr1121,
            queue,
            control,
            config,
            tx_in_flight: false,
            tx_burst_count: 0,
            rx_active: false,
            rx_payload_len: None,
            duty_cycle: DutyCycle::new(),
        };

        radio.apply_hardware_config()?;
        radio.control.publish_config(radio.config);
        Ok(radio)
    }

    fn apply_hardware_config(&mut self) -> Result<(), RadioError> {
        let config = self.config;

        // Image calibration needs the external oscillator, which is available
        // once the TCXO has been configured during system startup.
        self.lr1121.set_standby(1)?;

        match config.frequency_band {
            FrequencyBand::SBand => self.lr1121.set_lr_fhss_packet_type()?,
            FrequencyBand::SubGhz | FrequencyBand::Ism2_4Ghz => {
                self.lr1121.set_lora_packet_type()?;
            }
        }

        // The image-calibration range is encoded in 4 MHz steps and only
        // covers sub-GHz frequencies on the LR1121.
        if config.frequency_hz < 1_000_000_000 {
            let center_mhz = config.frequency_hz / 1_000_000;
            self.lr1121.calibrate_image_in_mhz(
                center_mhz.saturating_sub(10),
                center_mhz.saturating_add(10),
            )?;
        }

        self.lr1121.set_rf_frequency(config.frequency_hz)?;
        self.lr1121
            .set_rssi_calibration_for_frequency(config.frequency_hz)?;

        match config.frequency_band {
            FrequencyBand::SBand => {
                self.lr1121.set_lr_fhss_modulation_params()?;
                self.lr1121.set_lr_fhss_sync_word()?;
            }
            FrequencyBand::SubGhz | FrequencyBand::Ism2_4Ghz => {
                self.lr1121.set_lora_modulation_params(
                    config.spreading_factor,
                    config
                        .bandwidth
                        .lr1121_encoding(config.frequency_band)
                        .ok_or(RadioError::InvalidConfiguration)?,
                    config.coding_rate.register_value(),
                    config.low_data_rate_optimization_enabled(),
                )?;

                let initial_payload_length = if config.implicit_header { 1 } else { 0 };
                self.lr1121.set_lora_packet_params(
                    config.preamble_symbols,
                    config.implicit_header,
                    initial_payload_length,
                    config.crc_enabled,
                    config.invert_iq,
                )?;
                self.lr1121.set_lora_sync_word(config.lora_sync_word)?;
            }
        }

        let (pa_select, supply, duty_cycle, high_power_setup) =
            config.pa_mode.lr1121_register_values();
        self.lr1121
            .set_pa_config(pa_select, supply, duty_cycle, high_power_setup)?;

        self.lr1121
            .set_tx_params(config.tx_power_dbm, TxRampTime::Us48 as u8)?;

        self.lr1121
            .set_rx_tx_fallback_mode(RxTxFallbackMode::StandbyRc as u8)?;
        self.lr1121.set_rx_boosted(false)?;

        self.lr1121.clear_irq(u32::MAX)?;
        self.lr1121.set_dio_irq_params(IRQ_DIO9_MASK, 0)?;

        Ok(())
    }

    pub fn send_packet(&self, packet: &PacketData) -> Result<(), RadioError> {
        let length = packet.encoded_len();
        let maximum = self.config.frequency_band.maximum_packet_size();
        if length > maximum {
            return Err(RadioError::PayloadTooLong { length, maximum });
        }

        self.queue.add_packet(packet).map_err(|error| match error {
            QueueError::Packet(error) => RadioError::Packet(error),
            QueueError::PacketTooLong { length, maximum } => {
                RadioError::PayloadTooLong { length, maximum }
            }
            QueueError::Full => RadioError::TxQueueFull,
        })
    }

    /// Owns the radio and services the TX queue and RX interrupts.
    ///
    /// Passes `None` for explicit-header RX or `Some(length)` for
    /// implicit-header RX.
    pub async fn run_worker<F>(
        mut self,
        implicit_rx_payload_len: Option<u8>,
        mut on_packet_received: F,
    ) -> Result<(), RadioError>
    where
        F: AsyncFnMut(Packet),
    {
        match (self.config.implicit_header, implicit_rx_payload_len) {
            (false, None) => self.rx_payload_len = None,
            (true, Some(length)) if length > 0 && usize::from(length) <= MAX_TX_PAYLOAD => {
                self.rx_payload_len = Some(length);
            }
            (true, _) => {
                return Err(RadioError::ImplicitPayloadLengthRequired);
            }
            _ => return Err(RadioError::InvalidConfiguration),
        }

        if self.queue.queued_packet_count() == 0 && self.can_receive() {
            self.start_receive_internal()?;
        }

        loop {
            if self.tx_in_flight {
                self.lr1121.wait_for_irq().await;

                let irq = self.lr1121.get_irq_status()?;

                if irq == 0 {
                    self.lr1121.wait_for_irq_low().await;
                    continue;
                }

                self.lr1121.clear_irq(irq)?;

                if irq & IRQ_TX_DONE != 0 {
                    self.tx_in_flight = false;

                    if self.can_receive() {
                        if self.queue.queued_packet_count() == 0 {
                            self.tx_burst_count = 0;
                            self.start_receive_internal()?;
                        } else if self.tx_burst_count >= TX_BURST_LIMIT {
                            self.listen_during_tx_burst(&mut on_packet_received).await?;
                        }
                    }
                } else if irq & IRQ_TIMEOUT != 0 {
                    self.tx_in_flight = false;
                    self.tx_burst_count = 0;

                    if self.queue.queued_packet_count() == 0 && self.can_receive() {
                        self.start_receive_internal()?;
                    }

                    return Err(RadioError::TxTimeout);
                } else {
                    self.lr1121.wait_for_irq_low().await;
                }

                continue;
            }

            match select(
                select(self.queue.next_packet(), self.lr1121.wait_for_irq()),
                self.control.next_frequency_change(),
            )
            .await
            {
                Either::First(Either::First(packet)) => {
                    if self.rx_active && self.lr1121.irq_is_high() {
                        self.process_rx(&mut on_packet_received).await?;
                    }

                    // Wait for transmit airtime before the frame reaches the modem.
                    let airtime_us = self.config.time_on_air_us(packet.len());
                    self.duty_cycle
                        .reserve(self.config.frequency_band, airtime_us)
                        .await;
                    // A frame sized for another band is dropped instead of
                    // stopping the worker.
                    match self.start_transmit(&packet) {
                        Ok(()) => {}
                        Err(RadioError::PayloadTooLong { length, maximum }) => defmt::warn!(
                            "Dropped queued frame: {} bytes exceeds {} on this band",
                            length,
                            maximum
                        ),
                        Err(error) => return Err(error),
                    }
                }

                Either::First(Either::Second(())) => {
                    self.process_rx(&mut on_packet_received).await?;

                    if self.lr1121.irq_is_high() {
                        self.lr1121.wait_for_irq_low().await;
                    }
                }

                Either::Second(frequency_hz) => self.change_frequency(frequency_hz, true)?,
            }
        }
    }

    fn start_transmit(&mut self, packet: &Packet) -> Result<(), RadioError> {
        let payload = packet.as_slice();

        if payload.is_empty() {
            return Err(RadioError::InvalidParameter("payload must not be empty"));
        }

        let maximum = self
            .config
            .frequency_band
            .maximum_packet_size()
            .min(MAX_TX_PAYLOAD);
        if payload.len() > maximum {
            return Err(RadioError::PayloadTooLong {
                length: payload.len(),
                maximum,
            });
        }

        if self.rx_active {
            self.lr1121.set_standby(0)?;
            self.rx_active = false;
        }

        if self.config.frequency_band == FrequencyBand::SBand {
            self.lr1121.build_lr_fhss_frame(payload)?;
        } else {
            self.lr1121.set_lora_packet_params(
                self.config.preamble_symbols,
                self.config.implicit_header,
                payload.len() as u8,
                self.config.crc_enabled,
                self.config.invert_iq,
            )?;
            self.lr1121.write_buffer(payload)?;
        }
        self.lr1121.clear_irq(u32::MAX)?;
        self.lr1121.set_tx(TX_TIMEOUT_TICKS)?;

        self.tx_in_flight = true;
        self.tx_burst_count += 1;
        Ok(())
    }

    async fn listen_during_tx_burst<F>(
        &mut self,
        on_packet_received: &mut F,
    ) -> Result<(), RadioError>
    where
        F: AsyncFnMut(Packet),
    {
        self.tx_burst_count = 0;
        self.start_receive_internal()?;

        match select(self.lr1121.wait_for_irq(), Timer::after(RX_WINDOW)).await {
            Either::First(()) => {
                if self.lr1121.irq_is_high() {
                    self.process_rx(on_packet_received).await?;
                }
            }
            Either::Second(()) => {}
        }

        if self.rx_active {
            self.lr1121.set_standby(0)?;
            self.rx_active = false;
        }

        Ok(())
    }

    pub fn is_tx_idle(&self) -> bool {
        !self.tx_in_flight && self.queue.queued_packet_count() == 0
    }

    pub fn pending_tx_packets(&self) -> usize {
        self.queue.queued_packet_count() + usize::from(self.tx_in_flight)
    }

    pub fn start_receive(&mut self) -> Result<(), RadioError> {
        if !self.can_receive() {
            return Err(RadioError::InvalidConfiguration);
        }

        if self.tx_in_flight || self.queue.queued_packet_count() != 0 {
            return Err(RadioError::RadioBusy);
        }

        if self.config.implicit_header {
            return Err(RadioError::ImplicitPayloadLengthRequired);
        }

        self.rx_payload_len = None;
        self.start_receive_internal()
    }

    pub fn start_receive_implicit(&mut self, payload_len: u8) -> Result<(), RadioError> {
        if !self.can_receive() {
            return Err(RadioError::InvalidConfiguration);
        }

        if self.tx_in_flight || self.queue.queued_packet_count() != 0 {
            return Err(RadioError::RadioBusy);
        }

        if !self.config.implicit_header
            || payload_len == 0
            || usize::from(payload_len) > MAX_TX_PAYLOAD
        {
            return Err(RadioError::InvalidConfiguration);
        }

        self.rx_payload_len = Some(payload_len);
        self.start_receive_internal()
    }

    fn start_receive_internal(&mut self) -> Result<(), RadioError> {
        if !self.can_receive() {
            return Err(RadioError::InvalidConfiguration);
        }

        if self.rx_active {
            self.lr1121.set_standby(0)?;
            self.rx_active = false;
        }

        let (implicit_header, payload_len) = match self.rx_payload_len {
            Some(length) => (true, length),
            None => (false, 0),
        };

        self.lr1121.set_lora_packet_params(
            self.config.preamble_symbols,
            implicit_header,
            payload_len,
            self.config.crc_enabled,
            self.config.invert_iq,
        )?;

        self.lr1121.clear_irq(u32::MAX)?;
        self.lr1121.set_rx(0)?;
        self.rx_active = true;

        Ok(())
    }

    // Reads a received packet and passes its encoded bytes to the callback.
    pub async fn process_rx<F>(&mut self, mut on_packet_received: F) -> Result<bool, RadioError>
    where
        F: AsyncFnMut(Packet),
    {
        if !self.rx_active || self.tx_in_flight {
            return Ok(false);
        }

        let irq = self.lr1121.get_irq_status()?;

        if irq == 0 {
            return Ok(false);
        }

        if irq & (IRQ_HEADER_ERROR | IRQ_CRC_ERROR) != 0 {
            self.lr1121.clear_irq(irq)?;
            self.lr1121.set_rx(0)?;
            return Ok(false);
        }

        if irq & IRQ_RX_DONE == 0 {
            self.lr1121.clear_irq(irq)?;
            self.lr1121.set_rx(0)?;
            return Ok(false);
        }

        let status = self.lr1121.get_rx_buffer_status()?;
        let length = usize::from(status.payload_len);

        if length > MAX_PACKET_SIZE {
            self.lr1121.clear_irq(irq)?;
            self.lr1121.set_rx(0)?;

            return Err(RadioError::PayloadTooLong {
                length,
                maximum: MAX_PACKET_SIZE,
            });
        }

        let mut payload = [0u8; MAX_PACKET_SIZE];

        if length > 0 {
            self.lr1121
                .read_buffer(status.buffer_offset, &mut payload[..length])?;
        }

        let packet =
            Packet::from_slice(&payload[..length]).map_err(|_| RadioError::PayloadTooLong {
                length,
                maximum: MAX_PACKET_SIZE,
            })?;

        self.lr1121.clear_irq(irq)?;
        self.lr1121.set_rx(0)?;
        self.rx_active = true;
        on_packet_received(packet).await;

        Ok(true)
    }

    pub fn tx_done(&mut self) -> Result<bool, RadioError> {
        let irq = self.lr1121.get_irq_status()?;

        if irq & IRQ_TX_DONE != 0 {
            self.lr1121.clear_irq(IRQ_TX_DONE)?;
            self.tx_in_flight = false;
            return Ok(true);
        }

        if self.tx_in_flight && irq & IRQ_TIMEOUT != 0 {
            self.lr1121.clear_irq(IRQ_TIMEOUT)?;
            self.tx_in_flight = false;
            return Err(RadioError::TxTimeout);
        }

        Ok(false)
    }

    pub fn last_packet_status(&mut self) -> Result<PacketStatus, RadioError> {
        self.lr1121.get_packet_status()
    }

    // Changes RF band and modem after the radio worker has reached an idle boundary.
    pub fn set_frequency(&mut self, frequency_hz: u32) -> Result<(), RadioError> {
        if self.tx_in_flight || self.queue.queued_packet_count() != 0 {
            return Err(RadioError::RadioBusy);
        }

        self.change_frequency(frequency_hz, self.rx_active)
    }

    fn change_frequency(
        &mut self,
        frequency_hz: u32,
        resume_receive: bool,
    ) -> Result<(), RadioError> {
        let frequency_band =
            FrequencyBand::for_frequency(frequency_hz).ok_or(RadioError::InvalidConfiguration)?;
        let mut next_config = self.config;
        next_config.frequency_hz = frequency_hz;
        next_config.frequency_band = frequency_band;
        next_config.pa_mode = match frequency_band {
            FrequencyBand::SubGhz if self.config.pa_mode == PaMode::HighPower => PaMode::HighPower,
            FrequencyBand::SubGhz => PaMode::LowPower,
            FrequencyBand::SBand | FrequencyBand::Ism2_4Ghz => PaMode::HighFrequencyPath,
        };
        if frequency_band != FrequencyBand::SBand
            && next_config
                .bandwidth
                .lr1121_encoding(frequency_band)
                .is_none()
        {
            next_config.bandwidth = LoraBandwidth::Khz125;
        }
        if next_config.pa_mode == PaMode::HighFrequencyPath {
            next_config.tx_power_dbm = next_config.tx_power_dbm.min(13);
        }
        next_config
            .validate()
            .map_err(|_| RadioError::InvalidConfiguration)?;

        if self.rx_active {
            self.lr1121.set_standby(0)?;
            self.rx_active = false;
        }
        self.tx_burst_count = 0;

        let previous_config = self.config;
        self.config = next_config;
        if let Err(error) = self.apply_hardware_config() {
            self.config = previous_config;
            self.apply_hardware_config()?;
            if resume_receive && self.can_receive() {
                self.start_receive_internal()?;
            }
            return Err(error);
        }

        self.control.publish_config(self.config);

        if resume_receive && self.can_receive() {
            self.start_receive_internal()?;
        }

        Ok(())
    }

    fn can_receive(&self) -> bool {
        self.config.frequency_band != FrequencyBand::SBand
    }
}
