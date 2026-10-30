use crate::network::types::{DiscoveryList, DiscoverySnapshot};
use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex,
    watch::{Receiver, Watch},
};

/// 1/ Keep bounded lists of peers heard on the air, announced or passive.
pub struct DeviceDiscovery {
    devices: Watch<CriticalSectionRawMutex, DiscoverySnapshot, 1>,
}

impl DeviceDiscovery {
    /// 2/ Start with empty lists that the UI can read immediately.
    pub const fn new() -> Self {
        Self {
            devices: Watch::new_with(DiscoverySnapshot::new()),
        }
    }

    /// 3/ Record a peer heard in any accepted packet.
    pub fn remember(&self, device_id: [u8; 16]) {
        let sender = self.devices.sender();
        let mut snapshot = sender.try_get().unwrap_or_default();
        remember_device(&mut snapshot.heard, device_id);
        sender.send(snapshot);
    }

    /// 3/ Record a peer that announced itself, for the active search list.
    pub fn remember_announce(&self, device_id: [u8; 16]) {
        let sender = self.devices.sender();
        let mut snapshot = sender.try_get().unwrap_or_default();
        remember_device(&mut snapshot.heard, device_id);
        remember_device(&mut snapshot.announced, device_id);
        sender.send(snapshot);
    }

    /// 4/ Give the UI a live view of both lists.
    pub fn receiver(&self) -> Option<Receiver<'_, CriticalSectionRawMutex, DiscoverySnapshot, 1>> {
        self.devices.receiver()
    }
}

impl Default for DeviceDiscovery {
    fn default() -> Self {
        Self::new()
    }
}

fn remember_device(devices: &mut DiscoveryList, device_id: [u8; 16]) {
    if let Some(index) = devices.iter().position(|known| *known == device_id) {
        devices.remove(index);
    }
    if devices.is_full() {
        devices.remove(0);
    }
    let _ = devices.push(device_id);
}
