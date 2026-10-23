// Stores packet RSSI and SNR values in signed half-dB and quarter-dB units.
// Converts the raw RSSI value from half-dB units into dBm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PacketStatus {
    pub rssi_packet_half_db: i16,
    pub snr_quarter_db: i8,
    pub signal_rssi_half_db: i16,
}
