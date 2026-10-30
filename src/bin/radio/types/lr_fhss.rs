#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum LrFhssCodingRate {
    Cr5_6 = 0x00,
    Cr2_3 = 0x01,
    Cr1_2 = 0x02,
    Cr1_3 = 0x03,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum LrFhssModulation {
    Gmsk = 0x00,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum LrFhssGridSpacing {
    Fcc = 0x00,
    NonFcc = 0x01,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum LrFhssHopping {
    Disabled = 0x00,
    Enabled = 0x01,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum LrFhssBandwidth {
    Khz39_06 = 0x00,
    Khz85_94 = 0x01,
    Khz136_72 = 0x02,
    Khz183_59 = 0x03,
    Khz335_94 = 0x04,
    Khz386_72 = 0x05,
    Khz722_66 = 0x06,
    Khz773_44 = 0x07,
    Khz1523_4 = 0x08,
    Khz1574_2 = 0x09,
}

impl LrFhssBandwidth {
    pub const fn hz(self) -> u32 {
        match self {
            Self::Khz39_06 => 39_060,
            Self::Khz85_94 => 85_940,
            Self::Khz136_72 => 136_720,
            Self::Khz183_59 => 183_590,
            Self::Khz335_94 => 335_940,
            Self::Khz386_72 => 386_720,
            Self::Khz722_66 => 722_660,
            Self::Khz773_44 => 773_440,
            Self::Khz1523_4 => 1_523_400,
            Self::Khz1574_2 => 1_574_200,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum LrFhssHeaderCount {
    One = 1,
    Two = 2,
    Three = 3,
    Four = 4,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum LrFhssBitRate {
    Default = 0x8001_E848,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum LrFhssShaping {
    GaussianBt1 = 0x0B,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum LrFhssSyncWord {
    Default = 0x2C0F_7995,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum LrFhssHopSeed {
    Default = 0x013A,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i8)]
pub enum LrFhssDeviceOffset {
    Neutral = 0,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LrFhssProfile {
    SBandDefault,
}

impl LrFhssProfile {
    pub const fn header_count(self) -> LrFhssHeaderCount {
        match self {
            Self::SBandDefault => LrFhssHeaderCount::Two,
        }
    }

    pub const fn coding_rate(self) -> LrFhssCodingRate {
        match self {
            Self::SBandDefault => LrFhssCodingRate::Cr5_6,
        }
    }

    pub const fn modulation(self) -> LrFhssModulation {
        match self {
            Self::SBandDefault => LrFhssModulation::Gmsk,
        }
    }

    pub const fn grid_spacing(self) -> LrFhssGridSpacing {
        match self {
            Self::SBandDefault => LrFhssGridSpacing::NonFcc,
        }
    }

    pub const fn hopping(self) -> LrFhssHopping {
        match self {
            Self::SBandDefault => LrFhssHopping::Enabled,
        }
    }

    pub const fn bandwidth(self) -> LrFhssBandwidth {
        match self {
            Self::SBandDefault => LrFhssBandwidth::Khz136_72,
        }
    }

    pub const fn max_payload_size(self) -> usize {
        match self {
            Self::SBandDefault => 133,
        }
    }

    pub const fn hop_seed(self) -> LrFhssHopSeed {
        match self {
            Self::SBandDefault => LrFhssHopSeed::Default,
        }
    }

    pub const fn device_offset(self) -> LrFhssDeviceOffset {
        match self {
            Self::SBandDefault => LrFhssDeviceOffset::Neutral,
        }
    }
}
