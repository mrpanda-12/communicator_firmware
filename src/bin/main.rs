#![no_std]
#![no_main]
#![deny(clippy::mem_forget)]
#![deny(clippy::large_stack_frames)]

pub mod crypto;
pub mod hardware;
pub mod network;
pub mod packet;
pub mod radio;
pub mod storage;
pub mod ui;

static GPS_SNAPSHOT: Watch<CriticalSectionRawMutex, ui::GpsSnapshot, 2> = Watch::new();

const FIRMWARE_VERSION: &str = "0.0.6";
const RADIO_CONFIG: RadioConfig = RadioConfig {
    frequency_band: FrequencyBand::SubGhz,
    bandwidth: LoRaBandwidth::Khz125,
    pa_mode: PaMode::HighPower,
    tx_power_dbm: 22,
    ..RadioConfig::new(869_400_000)
};

use crate::{
    hardware::{flash::FlashStore, gps::Gps, lr1121::lr1121::Lr1121, pinout},
    network::{
        discovery::DeviceDiscovery,
        message_delivery::IncomingMessageQueue,
        packet_handler::PacketHandler,
        pairing::PairingCoordinator,
    },
    packet::{
        packet_data::{DEFAULT_HOP_LIMIT, PacketData},
        packet_type::PacketType,
    },
    radio::{
        control::RadioControl,
        queue::RadioQueue,
        radio::Radio,
        types::{FrequencyBand, LoRaBandwidth, PaMode, RadioConfig},
    },
    storage::storage_manager::StorageManager,
};

use embassy_executor::Spawner;
use embassy_futures::select::{Either, select};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, watch::Watch};
use embassy_time::{Duration, Timer};
use embedded_hal_bus::spi::ExclusiveDevice;
use esp_backtrace as _;
// Keep esp-println's defmt-espflash global logger linked for defmt output.
use esp_hal::{
    clock::CpuClock,
    gpio::{DriveMode, Level, Output, OutputConfig},
    ledc::{
        LSGlobalClkSource, Ledc, LowSpeed,
        channel::{self, ChannelIFace},
        timer::{self, TimerIFace},
    },
    peripherals,
    rng::{Trng, TrngSource},
    spi::{
        Mode,
        master::{Config, Spi},
    },
    time::Rate,
    timer::timg::TimerGroup,
};
#[allow(unused_imports)]
use esp_println::defmt as _;

extern crate alloc;

esp_bootloader_esp_idf::esp_app_desc!();

// 1/ Match these bounds to the `storage` row in the root partition table.
// 2/ Keep the end exclusive and both addresses aligned to 4 KiB erase pages.
const FLASH_DATA_START: u32 = 0x210000;
const FLASH_DATA_END: u32 = 0x400000;

// TFT uses SPI3 on GPIO12=SCK and GPIO11=MOSI.

/// 1/ Build the display's dedicated hardware SPI3 bus.
fn init_display_spi(
    spi3: peripherals::SPI3<'static>,
    sck: peripherals::GPIO12<'static>,
    mosi: peripherals::GPIO11<'static>,
) -> Spi<'static, esp_hal::Blocking> {
    Spi::new(
        spi3,
        Config::default()
            .with_frequency(Rate::from_mhz(10))
            .with_mode(Mode::_0),
    )
        .expect("Failed to initialize display SPI3")
        .with_sck(sck)
        .with_mosi(mosi)
}

/// 2/ Open the flash map over the reserved range from the real partition layout.
fn init_flash_storage(flash: peripherals::FLASH<'static>) -> FlashStore<'static> {
    FlashStore::new(flash, FLASH_DATA_START..FLASH_DATA_END)
        .expect("Invalid flash data partition range")
}

/// 3/ Keep the ADC entropy source alive for every use of the ESP-HAL TRNG.
fn init_trng(
    rng: peripherals::RNG<'static>,
    adc1: peripherals::ADC1<'static>,
) -> (TrngSource<'static>, Trng) {
    let source = TrngSource::new(rng, adc1);
    let trng = Trng::try_new().expect("Could not initialize ESP-HAL TRNG");
    (source, trng)
}

/// 4/ Load the device ID and create it only when the flash store has no ID.
async fn initialize_storage(
    flash: FlashStore<'static>,
    trng: &Trng,
    flash_buffer: &mut [u8],
) -> (StorageManager<'static>, Option<[u8; 16]>) {
    let mut storage = StorageManager::from_flash(flash);

    let device_id = match storage.load_device_id(flash_buffer).await {
        Ok(Some(device_id)) => {
            defmt::info!("Loaded device identity from internal flash");
            Some(device_id)
        }
        Ok(None) => match storage
            .regenerate_device_id(flash_buffer, |id| {
                trng.read(id);
                Ok(())
            })
            .await
        {
            Ok(device_id) => {
                defmt::info!("Generated and saved first-boot device identity");
                Some(device_id)
            }
            Err(_) => {
                defmt::error!("Failed to create and persist device identity");
                None
            }
        },
        Err(_) => {
            defmt::error!("Could not load device identity from internal flash");
            None
        }
    };

    (storage, device_id)
}

fn init_radio<'q>(
    spi2: peripherals::SPI2<'static>,
    gpio40: peripherals::GPIO40<'static>,
    gpio45: peripherals::GPIO45<'static>,
    gpio46: peripherals::GPIO46<'static>,
    gpio42: peripherals::GPIO42<'static>,
    gpio41: peripherals::GPIO41<'static>,
    gpio39: peripherals::GPIO39<'static>,
    gpio38: peripherals::GPIO38<'static>,
    radio_queue: &'q RadioQueue,
    radio_control: &'q RadioControl,
) -> Radio<'static, 'q, Spi<'static, esp_hal::Blocking>> {
    let radio_pins = pinout::radio_pins(gpio40, gpio45, gpio46, gpio42, gpio41, gpio39, gpio38);

    let spi = Spi::new(
        spi2,
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

    Radio::new(RADIO_CONFIG, lr1121, radio_queue, radio_control)
        .expect("Failed to configure LR1121")
}

fn init_gps(
    uart1: peripherals::UART1<'static>,
    gpio4: peripherals::GPIO4<'static>,
    gpio5: peripherals::GPIO5<'static>,
) -> Gps<'static> {
    let gps_pins = pinout::gps_pins(uart1, gpio4, gpio5);
    Gps::new(gps_pins.uart, gps_pins.rx, gps_pins.tx).expect("Failed to initialize GPS")
}

async fn gps_task(mut gps: Gps<'static>) -> ! {
    let mut published = ui::gps_snapshot(None, None);
    GPS_SNAPSHOT.sender().send(published);

    loop {
        let _ = gps.update();
        let snapshot = ui::gps_snapshot(gps.get_coordinate(), gps.get_time());
        if snapshot != published {
            GPS_SNAPSHOT.sender().send(snapshot);
            published = snapshot;
        }
        Timer::after(Duration::from_millis(20)).await;
    }
}

#[esp_rtos::main]
async fn main(_spawner: Spawner) -> ! {
    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);

    esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 73744);

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    esp_rtos::start(timg0.timer0, peripherals.FROM_CPU_INTR0);

    // Do not allow storage to start until the actual partition range is set.
    // FlashStore may erase pages inside this range as part of normal writes.
    if FLASH_DATA_START == 0 || FLASH_DATA_END <= FLASH_DATA_START {
        defmt::error!("Storage disabled: set FLASH_DATA_START/END from the real partition table");
        loop {
            Timer::after(Duration::from_secs(1)).await;
        }
    }

    // 5/ Initialize custom storage backends and persistent application values.
    let flash_store = init_flash_storage(peripherals.FLASH);
    let (_trng_source, trng) = init_trng(peripherals.RNG, peripherals.ADC1);
    let mut flash_buffer = [0u8; 512];
    let (mut storage, device_id) = initialize_storage(flash_store, &trng, &mut flash_buffer).await;
    let display_brightness =
        ui::display_settings::load_brightness(&mut storage, &mut flash_buffer).await;
    let mut contacts = ui::contact::ContactBook::new();
    match storage.load_contacts(&mut flash_buffer).await {
        Ok(saved_contacts) => {
            for contact in saved_contacts {
                if Some(contact.device_id) != device_id {
                    let _ = contacts.add_paired_direct(
                        contact.device_id,
                        contact.custom_name.as_str(),
                        contact.key,
                    );
                }
            }
        }
        Err(_) => defmt::warn!("Could not load saved contacts from flash"),
    }

    let mut display_buffer = [0u8; 512];
    let display_cs = Output::new(peripherals.GPIO10, Level::High, OutputConfig::default());
    let display_dc = Output::new(peripherals.GPIO9, Level::Low, OutputConfig::default());
    let display_reset = Output::new(peripherals.GPIO8, Level::High, OutputConfig::default());
    let display_bus = init_display_spi(peripherals.SPI3, peripherals.GPIO12, peripherals.GPIO11);
    let display_spi = ExclusiveDevice::new(display_bus, display_cs, embassy_time::Delay)
        .expect("Failed to create display SPI device");
    let display = hardware::display::init(
        display_spi,
        display_dc,
        display_reset,
        embassy_time::Delay,
        &mut display_buffer,
    );
    // 6/ Drive the module's active-high backlight input with hardware PWM.
    let mut ledc = Ledc::new(peripherals.LEDC);
    ledc.set_global_slow_clock(LSGlobalClkSource::APBClk);
    let mut backlight_timer = ledc.timer::<LowSpeed>(timer::Number::Timer0);
    backlight_timer
        .configure(timer::config::Config {
            duty: timer::config::Duty::Duty10Bit,
            clock_source: timer::LSClockSource::APBClk,
            frequency: Rate::from_khz(20),
        })
        .expect("Failed to configure display backlight PWM timer");
    let mut display_backlight =
        ledc.channel::<LowSpeed>(channel::Number::Channel0, peripherals.GPIO7);
    display_backlight
        .configure(channel::config::Config {
            timer: &backlight_timer,
            duty_pct: display_brightness,
            drive_mode: DriveMode::PushPull,
        })
        .expect("Failed to configure display backlight PWM channel");

    let radio_queue = RadioQueue::new();
    let radio_control = RadioControl::new();
    let discovery = DeviceDiscovery::new();
    let incoming_messages = IncomingMessageQueue::new();
    let pairing = PairingCoordinator::new();
    let radio = init_radio(
        peripherals.SPI2,
        peripherals.GPIO40,
        peripherals.GPIO45,
        peripherals.GPIO46,
        peripherals.GPIO42,
        peripherals.GPIO41,
        peripherals.GPIO39,
        peripherals.GPIO38,
        &radio_queue,
        &radio_control,
    );
    let gps = init_gps(peripherals.UART1, peripherals.GPIO4, peripherals.GPIO5);

    let mut packet_handler = PacketHandler::new();
    let radio_worker = radio.run_worker(None, async |packet| {
        packet_handler
            .handle_packet(
                packet,
                &radio_queue,
                &discovery,
                &incoming_messages,
                &pairing,
                &mut storage,
                &mut flash_buffer,
            )
            .await;
    });
    let gps_fut = gps_task(gps);
    let test_packet_producer = async {
        let mut packet_id = 1u16;

        loop {
            let Some(device_id) = device_id else {
                defmt::warn!("Skipping test packet: device identity is unavailable");
                Timer::after(Duration::from_secs(30)).await;
                continue;
            };
            defmt::info!("Device identity: {}", device_id);
            defmt::info!("Packet {:x}", packet_id);

            let packet = PacketData::new(
                packet_id,
                packet_id,
                0,
                1,
                device_id,
                [0; 16],
                false,
                PacketType::Text,
                b"Hello from the communicator",
                DEFAULT_HOP_LIMIT,
                DEFAULT_HOP_LIMIT,
            )
                .expect("valid test packet");

            if radio_queue.add_packet(&packet).is_err() {
                defmt::warn!("Test packet queue is full; dropping packet");
            }

            packet_id = packet_id.wrapping_add(1);
            Timer::after(Duration::from_secs(30)).await;
        }
    };

    let services_fut = async {
        match select(radio_worker, gps_fut).await {
            Either::First(Ok(())) => defmt::error!("Radio worker stopped"),
            Either::First(Err(_)) => defmt::error!("Radio worker failed"),
            Either::Second(_) => defmt::error!("GPS task stopped"),
        }
    };
    GPS_SNAPSHOT.sender().send(ui::gps_snapshot(None, None));
    let ui_fut = ui::run(
        display,
        device_id,
        contacts,
        FIRMWARE_VERSION,
        &pairing,
        &incoming_messages,
        pairing
            .event_receiver()
            .expect("Pairing event watch should be initialized"),
        discovery
            .receiver()
            .expect("Discovery watch should be initialized"),
        &radio_queue,
        radio_control
            .config_receiver()
            .expect("Radio config watch should be initialized"),
        GPS_SNAPSHOT
            .receiver()
            .expect("GPS snapshot watch should be initialized"),
    );
    let services_and_ui_fut = async {
        match select(services_fut, ui_fut).await {
            Either::First(()) => defmt::error!("Firmware service stopped"),
            Either::Second(never) => match never {},
        }
    };
    let _ = select(services_and_ui_fut, test_packet_producer).await;

    loop {
        Timer::after(Duration::from_secs(1)).await;
    }
}
