// Exposed ESP32-S3-LR1121-HF-Kit GPIO assignments. These values are GPIO numbers,
// not initialized peripheral pins; obtain the peripheral singleton in main.
pub const GPS_UART_RX_GPIO: u8 = 4;
pub const GPS_UART_TX_GPIO: u8 = 5;
pub const GPS_PPS_GPIO: u8 = 6;

pub const DISPLAY_BACKLIGHT_GPIO: u8 = 7;
pub const DISPLAY_RESET_GPIO: u8 = 8;
pub const DISPLAY_DC_GPIO: u8 = 9;
pub const DISPLAY_CS_GPIO: u8 = 10;
pub const DISPLAY_MOSI_GPIO: u8 = 11;
pub const DISPLAY_SCK_GPIO: u8 = 12;

pub const BUTTON_EXPANDER_INTERRUPT_GPIO: u8 = 16;
pub const BUTTON_EXPANDER_SDA_GPIO: u8 = 17;
pub const BUTTON_EXPANDER_SCL_GPIO: u8 = 18;

pub const LR1121_INTERRUPT_GPIO: u8 = 38;
pub const LR1121_RESET_GPIO: u8 = 39;
pub const LR1121_SCK_GPIO: u8 = 40;
pub const LR1121_BUSY_GPIO: u8 = 41;
pub const LR1121_CS_GPIO: u8 = 42;
pub const UART0_TX_GPIO: u8 = 43;
pub const UART0_RX_GPIO: u8 = 44;
pub const LR1121_MOSI_GPIO: u8 = 45;
pub const LR1121_MISO_GPIO: u8 = 46;
