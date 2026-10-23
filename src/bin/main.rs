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
    hardware::{gps::Gps, lr1121::lr1121::Lr1121, pinout},
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
    spi::{
        Mode,
        master::{Config, Spi},
    },
    time::Rate,
    timer::timg::TimerGroup,
};

extern crate alloc;

esp_bootloader_esp_idf::esp_app_desc!();

fn init_radio(peripherals: esp_hal::peripherals::Peripherals) -> Radio<'static, Spi<'static, esp_hal::Blocking>> {
    let radio_pins = pinout::radio_pins(
        peripherals.GPIO40,
        peripherals.GPIO45,
        peripherals.GPIO46,
        peripherals.GPIO42,
        peripherals.GPIO41,
        peripherals.GPIO39,
        peripherals.GPIO38,
    );

    let spi = Spi::new(
        peripherals.SPI2,
        Config::default()
            .with_frequency(Rate::from_mhz(1))
            .with_mode(Mode::_0),
    )
    .expect("Failed to initialize SPI2")
    .with_sck(radio_pins.spi_sck)
    .with_mosi(radio_pins.spi_mosi)
    .with_miso(radio_pins.spi_miso);

    let lr1121 = Lr1121::new(
        spi,
        radio_pins.nss,
        radio_pins.busy,
        radio_pins.reset,
        radio_pins.irq,
    )
    .expect("Failed to initialize LR1121");

    let radio_config = RadioConfig {
        frequency_band: FrequencyBand::SubGhz,
        bandwidth: LoRaBandwidth::Khz125,
        pa_mode: PaMode::HighPower,
        tx_power_dbm: 22,
        ..RadioConfig::new(869_400_000)
    };

    Radio::new(radio_config, lr1121).expect("Failed to configure LR1121")
}

fn init_gps() -> Gps<'static> {
    let peripherals = unsafe { esp_hal::peripherals::Peripherals::steal() };
    let gps_pins = pinout::gps_pins(peripherals.UART1, peripherals.GPIO4, peripherals.GPIO5);
    Gps::new(gps_pins.uart, gps_pins.rx, gps_pins.tx).expect("Failed to initialize GPS")
}

async fn gps_task(mut gps: Gps<'static>) -> ! {
    loop {
        let _ = gps.update();
        Timer::after(Duration::from_millis(250)).await;
    }
}

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

    let radio_periphs = unsafe { esp_hal::peripherals::Peripherals::steal() };
    let radio = init_radio(radio_periphs);
    let gps = init_gps();

    let radio_worker = radio.run_worker(None, |packet| PacketHandler::handle_packet(packet));
    let gps_fut = gps_task(gps);

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

    let fut = async {
        match select(radio_worker, gps_fut).await {
            Either::First(Ok(())) => defmt::error!("Radio worker stopped"),
            Either::First(Err(_)) => defmt::error!("Radio worker failed"),
            Either::Second(_) => defmt::error!("GPS task stopped"),
        }
    };
    let _ = select(fut, test_packet_producer).await;

    loop {
        Timer::after(Duration::from_secs(1)).await;
    }
}
