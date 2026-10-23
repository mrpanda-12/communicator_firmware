// Stores the status fields returned by the GetStatus command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RadioStatus {
    pub stat1: u8,
    pub stat2: u8,
    pub irq_status: u32,
}
