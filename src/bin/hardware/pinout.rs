// Board pin assignments for the Waveshare ESP32-S3-LR1121-HF-Kit.
// Keep the wiring here so main.rs does not duplicate GPIO numbers.

use esp_hal::{
    gpio::{Input, InputConfig, Level, Output, OutputConfig},
    peripherals::{GPIO4, GPIO5, GPIO38, GPIO39, GPIO40, GPIO41, GPIO42, GPIO45, GPIO46, UART1},
};

pub const DISPLAY_BACKLIGHT_GPIO: u8 = 7;
pub const DISPLAY_RESET_GPIO: u8 = 8;
pub const DISPLAY_DC_GPIO: u8 = 9;
pub const DISPLAY_CS_GPIO: u8 = 10;
pub const DISPLAY_MOSI_GPIO: u8 = 11;
pub const DISPLAY_SCK_GPIO: u8 = 12;

pub const BUTTON_EXPANDER_INTERRUPT_GPIO: u8 = 16;
pub const BUTTON_EXPANDER_SDA_GPIO: u8 = 17;
pub const BUTTON_EXPANDER_SCL_GPIO: u8 = 18;

pub const SDCARD_SPI_SCK_GPIO: u8 = 13;
pub const SDCARD_SPI_MOSI_GPIO: u8 = 14;
pub const SDCARD_SPI_MISO_GPIO: u8 = 15;
pub const SDCARD_CS_GPIO: u8 = 6;

pub const UART0_TX_GPIO: u8 = 43;
pub const UART0_RX_GPIO: u8 = 44;

pub struct RadioPins<'d> {
    pub spi_sck: GPIO40<'d>,
    pub spi_mosi: GPIO45<'d>,
    pub spi_miso: GPIO46<'d>,
    pub nss: Output<'d>,
    pub busy: Input<'d>,
    pub reset: Output<'d>,
    pub irq: Input<'d>,
}

// Configures the LR1121 SPI bus pins and control signals.
pub fn radio_pins<'d>(
    spi_sck: GPIO40<'d>,
    spi_mosi: GPIO45<'d>,
    spi_miso: GPIO46<'d>,
    nss: GPIO42<'d>,
    busy: GPIO41<'d>,
    reset: GPIO39<'d>,
    irq: GPIO38<'d>,
) -> RadioPins<'d> {
    RadioPins {
        spi_sck,
        spi_mosi,
        spi_miso,
        nss: Output::new(nss, Level::High, OutputConfig::default()),
        busy: Input::new(busy, InputConfig::default()),
        reset: Output::new(reset, Level::High, OutputConfig::default()),
        irq: Input::new(irq, InputConfig::default()),
    }
}

pub struct GpsPins<'d> {
    pub uart: UART1<'d>,
    pub rx: GPIO4<'d>,
    pub tx: GPIO5<'d>,
}

// UART1 uses GPIO4/GPIO5 and leaves UART0 free for programming and logs.
pub fn gps_pins<'d>(uart: UART1<'d>, rx: GPIO4<'d>, tx: GPIO5<'d>) -> GpsPins<'d> {
    GpsPins { uart, rx, tx }
}
