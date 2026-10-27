use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex,
    channel::Channel,
    watch::{Receiver, Watch},
};

use crate::radio::{
    radio_error::RadioError,
    types::{FrequencyBand, RadioConfig},
};

const CONTROL_CAPACITY: usize = 1;

/// 1/ Carry local control requests to one radio's worker.
pub struct RadioControl {
    frequency_changes: Channel<CriticalSectionRawMutex, u32, CONTROL_CAPACITY>,
    config_updates: Watch<CriticalSectionRawMutex, RadioConfig, 1>,
}

impl RadioControl {
    /// 2/ Create an empty control channel for one radio worker.
    pub const fn new() -> Self {
        Self {
            frequency_changes: Channel::new(),
            config_updates: Watch::new(),
        }
    }

    /// 3/ Ask this radio's worker to retune after it reaches a safe point.
    pub fn request_frequency_change(&self, frequency_hz: u32) -> Result<(), RadioError> {
        if FrequencyBand::for_frequency(frequency_hz).is_none() {
            return Err(RadioError::InvalidConfiguration);
        }

        self.frequency_changes
            .try_send(frequency_hz)
            .map_err(|_| RadioError::RadioBusy)
    }

    /// 4/ Let the owning worker wait for its next control request.
    pub(crate) async fn next_frequency_change(&self) -> u32 {
        self.frequency_changes.receive().await
    }

    /// 5/ Publish the radio's active config whenever the worker applies a change.
    pub(crate) fn publish_config(&self, config: RadioConfig) {
        self.config_updates.sender().send(config);
    }

    /// 6/ Give the UI a receiver for live frequency and transmit-power updates.
    pub fn config_receiver(&self) -> Option<Receiver<'_, CriticalSectionRawMutex, RadioConfig, 1>> {
        self.config_updates.receiver()
    }
}

impl Default for RadioControl {
    fn default() -> Self {
        Self::new()
    }
}
