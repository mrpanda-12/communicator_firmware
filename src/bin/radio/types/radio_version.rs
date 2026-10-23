// Stores the version fields returned by the GetVersion command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RadioVersion {
    pub hw_version: u8,
    pub use_case: u8,
    pub fw_major: u8,
    pub fw_minor: u8,
}
