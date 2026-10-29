use heapless::Vec;

/// Maximum device IDs kept in the discovered list.
pub const MAX_DISCOVERED_DEVICES: usize = 8;

/// Device IDs heard in accepted packets, oldest first.
pub type DiscoveryList = Vec<[u8; 16], MAX_DISCOVERED_DEVICES>;
