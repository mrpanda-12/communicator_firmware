#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum Opcode {
    GetStatus = 0x0100,        // Direct-read status bytes / clears reset status
    GetVersion = 0x0101,       // Reads firmware and hardware versions
    WriteBuffer = 0x0109,      // Writes bytes to the radio buffer
    ReadBuffer = 0x010A,       // Reads bytes from the radio buffer
    ClearRxBuffer = 0x010B,    // Clears the RX buffer
    GetErrors = 0x010D,        // Reads device error flags
    ClearErrors = 0x010E,      // Clears device error flags
    Calibrate = 0x010F,        // Runs the requested system calibrations
    SetRegMode = 0x0110,       // Selects LDO or DCDC regulator mode
    CalibrateImage = 0x0111,   // Calibrates image rejection for a frequency range
    SetDioAsRfSwitch = 0x0112, // Routes RF switch controls onto the DIO pins
    SetDioIrqParams = 0x0113,  // Routes IRQs to DIO9 and DIO11
    ClearIrq = 0x0114,         // Clears selected IRQ flags
    CfgLfclk = 0x0116,         // Selects the low-frequency clock source
    SetTcxoMode = 0x0117,      // Configures TCXO supply voltage and startup time
    SetStandby = 0x011C,       // Enters standby mode
    EnableSpiCrc = 0x0128,     // Enables or disables CRC over SPI

    GetPacketType = 0x0202,       // Reads the selected packet type
    GetRxBufferStatus = 0x0203,   // Reads RX length and buffer offset
    GetPacketStatus = 0x0204,     // Reads packet RSSI and signal metrics
    GetRssiInst = 0x0205,         // Reads instantaneous RSSI
    SetRx = 0x0209,               // Starts reception
    SetTx = 0x020A,               // Starts transmission
    SetRfFrequency = 0x020B,      // Sets the RF frequency
    SetPacketType = 0x020E,       // Selects the modem/packet type
    SetModulationParams = 0x020F, // Configures modulation
    SetPacketParams = 0x0210,     // Configures packet format
    SetTxParams = 0x0211,         // Sets TX power and ramp time
    SetRxTxFallbackMode = 0x0213, // Sets the post-TX/RX mode
    SetPaConfig = 0x0215,         // Configures the power amplifier
    SetRxBoosted = 0x0227,        // Enables the RX boost mode
    SetRssiCalibration = 0x0229,  // Loads the RSSI calibration table
    SetLoraSyncWord = 0x022B,     // Sets the LoRa sync word
    LrFhssBuildFrame = 0x022C,    // Builds an LR-FHSS transmit frame
    LrFhssSetSyncWord = 0x022D,   // Sets the LR-FHSS sync word
}

impl Opcode {
    // Encodes the 16-bit hardware opcode as big-endian SPI bytes.
    pub fn to_bytes(self) -> [u8; 2] {
        let val = self as u16;
        [(val >> 8) as u8, (val & 0xFF) as u8]
    }
}
