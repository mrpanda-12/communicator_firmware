//! MCP23017 I2C button-expander driver for the 5x5 keypad matrix.
//!
//! Matrix wiring (see `docs/pinout.md`): PA0-PA3 are rows 0-3 with pull-ups,
//! PA4-PA7 drive columns 0-3, PB0 drives column 4, and PB1 reads row 4. The
//! datasheet writes these ports as GPA0-GPA7 and GPB0-GPB7.
//! INTA (mirrored from both ports) drives GPIO16 and wakes the worker when any
//! row changes, so an idle keypad costs no I2C traffic.
//!
//! Columns rest low between interrupts, so a pressed key pulls its row low and
//! latches an interrupt. A scan drives one column low and the other four high,
//! reads the rows, then restores the idle state and clears the latch.

use core::ops::AsyncFnMut;

use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};
use embassy_time::{Duration, Timer};
use embedded_hal::i2c::I2c;
use esp_hal::gpio::Input;

/// Default MCP23017 address with A0-A2 tied low.
pub const ADDRESS: u8 = 0x20;

// Register addresses for BANK = 0, the reset default.
const REG_IODIR_A: u8 = 0x00;
const REG_IODIR_B: u8 = 0x01;
const REG_IPOL_A: u8 = 0x02;
const REG_IPOL_B: u8 = 0x03;
const REG_GPINTEN_A: u8 = 0x04;
const REG_GPINTEN_B: u8 = 0x05;
const REG_INTCON_A: u8 = 0x08;
const REG_INTCON_B: u8 = 0x09;
const REG_IOCON: u8 = 0x0A;
const REG_GPPU_A: u8 = 0x0C;
const REG_GPPU_B: u8 = 0x0D;
const REG_GPIO_A: u8 = 0x12;
const REG_GPIO_B: u8 = 0x13;
const REG_OLAT_A: u8 = 0x14;
const REG_OLAT_B: u8 = 0x15;

// IOCON: mirror both ports to INTA and drive it as an open-drain active-low
// output, so the ESP32 pull-up owns the idle level.
const IOCON_MIRROR: u8 = 0x40;
const IOCON_ODR: u8 = 0x04;
const IOCON_VALUE: u8 = IOCON_MIRROR | IOCON_ODR;

pub const ROWS: usize = 5;
pub const COLUMNS: usize = 5;

// PA0-PA3 read rows 0-3; PA4-PA7 drive columns 0-3.
const ROW_A_MASK: u8 = 0x0F;
const COLUMN_A_MASK: u8 = 0xF0;
// PB0 drives column 4; PB1 reads row 4.
const COLUMN_B_MASK: u8 = 0x01;
const ROW_B_MASK: u8 = 0x02;

/// How long one matrix cell must read the same before it counts as settled.
const DEBOUNCE_SCANS: u8 = 3;
const SCAN_INTERVAL: Duration = Duration::from_millis(10);
const RETRY_INTERVAL: Duration = Duration::from_millis(2_000);

/// One logical key on the 5x5 matrix.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeypadKey {
    /// Hex digit 0x0-0xF; two digits type one ASCII character.
    Digit(u8),
    Back,
    Send,
    Enter,
    Up,
    Down,
    Left,
    Right,
    /// Opens the on-screen keypad help; the full ASCII table comes later.
    Help,
    /// A cell with no assigned function.
    Unassigned,
}

/// Physical matrix layout, row 0 at the top. SEND is the first action key so
/// it sits at the top; the hex digits form a 4x4 block and HELP opens the
/// on-screen legend.
///
/// ```text
/// 0 1 2 3 SEND
/// 4 5 6 7 BACK
/// 8 9 A B ENTER
/// C D E F HELP
/// UP DOWN LEFT RIGHT -
/// ```
pub const LAYOUT: [[KeypadKey; COLUMNS]; ROWS] = [
    [
        KeypadKey::Digit(0x0),
        KeypadKey::Digit(0x1),
        KeypadKey::Digit(0x2),
        KeypadKey::Digit(0x3),
        KeypadKey::Send,
    ],
    [
        KeypadKey::Digit(0x4),
        KeypadKey::Digit(0x5),
        KeypadKey::Digit(0x6),
        KeypadKey::Digit(0x7),
        KeypadKey::Back,
    ],
    [
        KeypadKey::Digit(0x8),
        KeypadKey::Digit(0x9),
        KeypadKey::Digit(0xA),
        KeypadKey::Digit(0xB),
        KeypadKey::Enter,
    ],
    [
        KeypadKey::Digit(0xC),
        KeypadKey::Digit(0xD),
        KeypadKey::Digit(0xE),
        KeypadKey::Digit(0xF),
        KeypadKey::Help,
    ],
    [
        KeypadKey::Up,
        KeypadKey::Down,
        KeypadKey::Left,
        KeypadKey::Right,
        KeypadKey::Unassigned,
    ],
];

/// MCP23017-backed matrix scanner with per-press debouncing.
pub struct Keypad<I2C> {
    i2c: I2C,
    irq: Input<'static>,
    configured: bool,
    debounced: Option<KeypadKey>,
    candidate: Option<KeypadKey>,
    stable_scans: u8,
}

impl<I2C: I2c> Keypad<I2C> {
    /// 1/ Keep the bus and IRQ pin; configuration waits for the worker.
    pub const fn new(i2c: I2C, irq: Input<'static>) -> Self {
        Self {
            i2c,
            irq,
            configured: false,
            debounced: None,
            candidate: None,
            stable_scans: 0,
        }
    }

    /// 2/ Sleep on the expander interrupt and call back once per settled press.
    ///
    /// This mirrors `Radio::run_worker`: the worker owns the bus and the caller
    /// decides what a key means, so a missing expander only stops key events.
    pub async fn run_worker<F>(mut self, mut on_key: F) -> !
    where
        F: AsyncFnMut(KeypadKey),
    {
        let mut reported_failure = false;
        loop {
            if !self.configured {
                if self.configure().is_err() {
                    if !reported_failure {
                        defmt::warn!("MCP23017 keypad did not answer at address 0x20");
                        reported_failure = true;
                    }
                    Timer::after(RETRY_INTERVAL).await;
                    continue;
                }
                if reported_failure {
                    defmt::info!("MCP23017 keypad is responding again");
                    reported_failure = false;
                }
            }

            // Wait for a row change, unless the interrupt latched while we were
            // still settling the previous one.
            if self.is_settled() && !self.irq_low() {
                self.irq.wait_for_falling_edge().await;
            }

            // The edge only says "something changed"; debounce it with a short
            // burst of scans instead of polling forever.
            for _ in 0..DEBOUNCE_SCANS {
                Timer::after(SCAN_INTERVAL).await;
                match self.poll() {
                    Ok(Some(key)) => on_key(key).await,
                    Ok(None) => {}
                    Err(_) => {
                        self.configured = false;
                        break;
                    }
                }
            }
        }
    }

    /// 3/ Scan once and report a key only when a newly pressed key is stable.
    fn poll(&mut self) -> Result<Option<KeypadKey>, I2C::Error> {
        if !self.configured {
            self.configure()?;
        }
        let reading = match self.scan() {
            Ok(pressed) => pressed,
            Err(error) => {
                // Reconfigure before the next attempt so the expander is re-armed.
                self.configured = false;
                return Err(error);
            }
        };

        if reading == self.candidate {
            self.stable_scans = self.stable_scans.saturating_add(1);
        } else {
            self.candidate = reading;
            self.stable_scans = 1;
        }
        if self.stable_scans < DEBOUNCE_SCANS || self.candidate == self.debounced {
            return Ok(None);
        }
        self.debounced = self.candidate;
        Ok(match self.debounced {
            Some(KeypadKey::Unassigned) | None => None,
            Some(key) => Some(key),
        })
    }

    /// 4/ True when no key change is waiting for another scan.
    fn is_settled(&self) -> bool {
        self.candidate == self.debounced
    }

    fn irq_low(&mut self) -> bool {
        self.irq.is_low()
    }

    /// 5/ Rows become pulled-up interrupt inputs; columns idle low.
    fn configure(&mut self) -> Result<(), I2C::Error> {
        // Mirror INTB onto INTA and use an open-drain active-low output.
        self.write_register(REG_IOCON, IOCON_VALUE)?;
        self.write_register(REG_IODIR_A, ROW_A_MASK)?;
        self.write_register(REG_IODIR_B, !COLUMN_B_MASK)?;
        self.write_register(REG_IPOL_A, 0)?;
        self.write_register(REG_IPOL_B, 0)?;
        self.write_register(REG_GPPU_A, ROW_A_MASK)?;
        self.write_register(REG_GPPU_B, !COLUMN_B_MASK)?;
        // Interrupt on any change of a row, not a comparison against DEFVAL.
        self.write_register(REG_INTCON_A, 0)?;
        self.write_register(REG_INTCON_B, 0)?;
        self.write_register(REG_GPINTEN_A, ROW_A_MASK)?;
        self.write_register(REG_GPINTEN_B, ROW_B_MASK)?;
        self.idle_columns()?;
        self.clear_interrupt()?;
        self.configured = true;
        self.debounced = None;
        self.candidate = None;
        self.stable_scans = 0;
        Ok(())
    }

    /// 6/ Drive one column low, read every row, then return to idle and re-arm.
    fn scan(&mut self) -> Result<Option<KeypadKey>, I2C::Error> {
        let mut pressed = None;
        for column in 0..COLUMNS {
            self.drive_column(column)?;
            let rows_a = self.read_register(REG_GPIO_A)?;
            let rows_b = self.read_register(REG_GPIO_B)?;
            for row in 0..ROWS {
                let row_low = if row < 4 {
                    rows_a & (1 << row) == 0
                } else {
                    rows_b & ROW_B_MASK == 0
                };
                if row_low {
                    pressed = Some(LAYOUT[row][column]);
                    break;
                }
            }
            if pressed.is_some() {
                break;
            }
        }
        self.idle_columns()?;
        // Restoring the columns changes held rows again; read the ports once
        // more so the latch clears and the IRQ line returns to idle high.
        self.clear_interrupt()?;
        Ok(pressed)
    }

    /// 7/ Pull one column low while the other four stay high.
    fn drive_column(&mut self, column: usize) -> Result<(), I2C::Error> {
        let mut columns_a = COLUMN_A_MASK;
        if column < 4 {
            columns_a &= !(1 << (4 + column));
        }
        let columns_b = if column == 4 { 0 } else { COLUMN_B_MASK };
        self.write_register(REG_OLAT_A, columns_a)?;
        self.write_register(REG_OLAT_B, columns_b)
    }

    /// 8/ Leave every column low so any pressed key latches an interrupt.
    fn idle_columns(&mut self) -> Result<(), I2C::Error> {
        self.write_register(REG_OLAT_A, 0)?;
        self.write_register(REG_OLAT_B, 0)
    }

    /// 9/ Reading the ports clears the interrupt-on-change latch.
    fn clear_interrupt(&mut self) -> Result<(), I2C::Error> {
        self.read_register(REG_GPIO_A)?;
        self.read_register(REG_GPIO_B)?;
        Ok(())
    }

    fn read_register(&mut self, register: u8) -> Result<u8, I2C::Error> {
        let mut value = [0u8; 1];
        self.i2c.write_read(ADDRESS, &[register], &mut value)?;
        Ok(value[0])
    }

    fn write_register(&mut self, register: u8, value: u8) -> Result<(), I2C::Error> {
        self.i2c.write(ADDRESS, &[register, value])
    }
}

/// Key events waiting for the UI task, filled by the keypad worker callback.
/// Absorbs a burst of presses (for example a typed secret) before the UI
/// takes one; the queue lives in static storage, so depth costs little.
pub const KEYPAD_EVENT_CAPACITY: usize = 32;
pub type KeypadQueue = Channel<CriticalSectionRawMutex, KeypadKey, KEYPAD_EVENT_CAPACITY>;

/// Static hand-off from the keypad worker to the UI loop, like the radio and
/// incoming-message channels.
pub static KEYPAD_EVENTS: KeypadQueue = Channel::new();
