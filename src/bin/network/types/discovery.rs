use heapless::Vec;

/// Maximum device IDs kept in the discovered list.
pub const MAX_DISCOVERED_DEVICES: usize = 8;

/// Device IDs heard in accepted packets, oldest first.
pub type DiscoveryList = Vec<[u8; 16], MAX_DISCOVERED_DEVICES>;

/// Peers heard passively, and peers whose own announcement was heard.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DiscoverySnapshot {
    /// Every peer heard in any accepted packet.
    pub heard: DiscoveryList,
    /// Peers that sent a plaintext `DeviceAnnounce`.
    pub announced: DiscoveryList,
}

impl DiscoverySnapshot {
    pub const fn new() -> Self {
        Self {
            heard: Vec::new(),
            announced: Vec::new(),
        }
    }
}
