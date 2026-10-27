use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex,
    watch::{Receiver, Watch},
};
use heapless::Vec;

pub const MAX_DISCOVERED_DEVICES: usize = 8;
pub type DiscoveryList = Vec<[u8; 16], MAX_DISCOVERED_DEVICES>;

/// 1/ Keep a bounded list of device IDs heard in accepted packets.
pub struct DeviceDiscovery {
    devices: Watch<CriticalSectionRawMutex, DiscoveryList, 1>,
}

impl DeviceDiscovery {
    /// 2/ Start with an empty list that the UI can read immediately.
    pub const fn new() -> Self {
        Self {
            devices: Watch::new_with(Vec::new()),
        }
    }

    /// 3/ Record a heard device and move repeated IDs to the end.
    pub fn remember(&self, device_id: [u8; 16]) {
        let sender = self.devices.sender();
        let mut devices = sender.try_get().unwrap_or_default();
        remember_device(&mut devices, device_id);
        sender.send(devices);
    }

    /// 4/ Give the UI a live view of announced devices.
    pub fn receiver(&self) -> Option<Receiver<'_, CriticalSectionRawMutex, DiscoveryList, 1>> {
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

