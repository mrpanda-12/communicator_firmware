#![no_std]
#![no_main]
#![deny(clippy::mem_forget)]
#![deny(clippy::large_stack_frames)]

pub mod crypto;
pub mod device;
pub mod hardware;
pub mod network;
pub mod packet;
pub mod radio;
pub mod storage;

use crate::{
    hardware::lr1121::lr1121::Lr1121,
    network::packet_handler::PacketHandler,
    packet::{
        packet_data::{DEFAULT_HOP_LIMIT, PacketData},
        packet_type::PacketType,
    },
    radio::{
        queue,
        radio::Radio,
        types::{FrequencyBand, LoRaBandwidth, PaMode, RadioConfig},
    },
};
use embassy_executor::Spawner;
use embassy_futures::select::{Either, select};
use embassy_time::{Duration, Timer};
use esp_backtrace as _;
use esp_hal::{
    clock::CpuClock,
    gpio::{Input, InputConfig, Level, Output, OutputConfig},
    spi::{
        Mode,
        master::{Config, Spi},
    },
    time::Rate,
    timer::timg::TimerGroup,
};

extern crate alloc;

esp_bootloader_esp_idf::esp_app_desc!();

#[esp_rtos::main]
async fn main(_spawner: Spawner) -> ! {
    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);

    let _gpio27 = peripherals.GPIO27;
    let _gpio28 = peripherals.GPIO28;
    let _gpio29 = peripherals.GPIO29;
    let _gpio30 = peripherals.GPIO30;
    let _gpio31 = peripherals.GPIO31;
    let _gpio32 = peripherals.GPIO32;

    esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 73744);

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    esp_rtos::start(timg0.timer0, peripherals.FROM_CPU_INTR0);

    // Configures SPI2 for communication with the LR1121.
    let spi = Spi::new(
        peripherals.SPI2,
        Config::default()
            .with_frequency(Rate::from_mhz(1))
            .with_mode(Mode::_0),
    )
    .expect("Failed to initialize SPI2")
    .with_sck(peripherals.GPIO40)
    .with_mosi(peripherals.GPIO45)
    .with_miso(peripherals.GPIO46);

    // Configures the radio control pins.
    let nss = Output::new(peripherals.GPIO42, Level::High, OutputConfig::default());
    let busy = Input::new(peripherals.GPIO41, InputConfig::default());
    let reset = Output::new(peripherals.GPIO39, Level::High, OutputConfig::default());
    let irq = Input::new(peripherals.GPIO38, InputConfig::default());

    // Initializes the LR1121 radio driver.
    let lr1121 = Lr1121::new(spi, nss, busy, reset, irq).expect("Failed to initialize LR1121");

    let radio_config = RadioConfig {
        frequency_band: FrequencyBand::SubGhz,
        bandwidth: LoRaBandwidth::Khz125,
        pa_mode: PaMode::HighPower,
        tx_power_dbm: 22,
        ..RadioConfig::new(869_400_000)
    };

    let radio = Radio::new(radio_config, lr1121).expect("Failed to configure LR1121");

    let radio_worker = radio.run_worker(None, |packet| PacketHandler::handle_packet(packet));
    let test_packet_producer = async {
        let mut packet_id = 1u16;

        loop {
            let packet = PacketData::new(
                packet_id,
                packet_id,
                0,
                1,
                [1; 16],
                [0; 16],
                false,
                PacketType::Text,
                b"Hello from the communicator",
                DEFAULT_HOP_LIMIT,
                DEFAULT_HOP_LIMIT,
            )
            .expect("valid test packet");

            if queue::add_packet(&packet).is_err() {
                defmt::warn!("Test packet queue is full; dropping packet");
            }

            packet_id = packet_id.wrapping_add(1);
            Timer::after(Duration::from_secs(30)).await;
        }
    };

    match select(radio_worker, test_packet_producer).await {
        Either::First(Ok(())) => defmt::error!("Radio worker stopped"),
        Either::First(Err(_)) => defmt::error!("Radio worker failed"),
        Either::Second(()) => defmt::error!("Test packet producer stopped"),
    }

    loop {
        Timer::after(Duration::from_secs(1)).await;
    }
}
