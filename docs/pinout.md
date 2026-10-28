w# Pinout Plan

This is the proposed external wiring for the Waveshare ESP32-S3-LR1121-HF-Kit. GPIO assignments below use the 21 exposed module GPIOs from the supplied pinout image. Verify connector positions against the kit's schematic before soldering; this document is a wiring plan, not a confirmed board-layout drawing.

The USB adapter board is not used for these peripheral connections. Keep all signal levels at 3.3 V. A module advertised as accepting 5 V supply may still have 3.3 V-only logic; check its schematic and do not drive ESP32 GPIOs with 5 V.

## Existing Board Signals

| ESP32-S3 GPIO | Existing signal | Use |
| ---: | --- | --- |
| GPIO38 | LR1121_INT | Keep for radio interrupt. |
| GPIO39 | LR1121_RST | Keep for radio reset. |
| GPIO40 | LR1121_CLK | Keep for radio SPI clock. |
| GPIO41 | LR1121_BUSY | Keep for radio busy input. |
| GPIO42 | LR1121_CS | Keep for radio chip select. |
| GPIO45 | LR1121_MOSI | Keep for radio SPI data to radio. |
| GPIO46 | LR1121_MISO | Keep for radio SPI data from radio. |
| GPIO43 | U0TXD | Keep for UART0/programming/debug output. |
| GPIO44 | U0RXD | Keep for UART0/programming/debug input. |

These radio assignments match the current firmware in `src/bin/main.rs` and Waveshare's published examples. Do not share the LR1121's chip-select line with another SPI device.

The LR1121 RF switch is controlled by the radio's own DIO pins through `SetDioAsRfSwitch`; it does not require any of the external GPIOs listed below. The board front end must still physically support the selected frequency band.

## Peripheral Wiring

The GPS and TFT wiring below is configured in the current firmware. The MCP23017 matrix wiring also matches `hardware/keypad.rs`; the expander has not been tested on hardware yet, and the firmware keeps running (with no keypad input) when it does not answer.

### u-blox NEO-M6N GPS

Use UART1, leaving UART0 available for programming and logs.

| ESP32-S3 | GPS module | Notes |
| --- | --- | --- |
| GPIO4 / UART1 RX | GPS TX | Cross TX to RX. |
| GPIO5 / UART1 TX | GPS RX | Cross RX to TX. |
| 3V3 or module-rated supply | VCC | Confirm the exact NEO-M6N breakout's input range and current before selecting the rail. |
| GND | GND | Common ground. |

Start with the module's documented baud rate and 8-N-1 framing; do not assume the receiver is still at its factory baud if it has been configured before. Confirm UART RX/TX with a serial monitor before adding GPS parsing.

The firmware wrapper is [`hardware/gps.rs`](../src/bin/hardware/gps.rs). It defaults to 9600 baud and uses the `nmea` crate in `no_std` mode with GGA/RMC parsing enabled. Create a `Gps` object, call `update()` regularly to drain buffered UART bytes and parse sentences, then read `gps.coordinate` and `gps.time` (or `get_coordinate()`/`get_time()`). `poll()` does not wait for input.

```rust
let gps_pins = pinout::gps_pins(
	peripherals.UART1,
	peripherals.GPIO4,
	peripherals.GPIO5,
);
let mut gps = Gps::new(gps_pins.uart, gps_pins.rx, gps_pins.tx)?;
gps.update()?;
if let Some(coord) = gps.coordinate {
    // use coord
}
```

`get_coordinate()` and `get_time()` return the most recently parsed valid data as `Option`; malformed/checksum-invalid sentences are ignored and exposed through `last_sentence_error()`. The supported GPS module has no PPS output, so the pin map does not reserve one and PPS timestamps are not implemented.

### ILI9341 2.4-Inch SPI TFT

Use hardware SPI3 for the display; SPI2 remains dedicated to the LR1121. The current firmware does not use the TFT's microSD socket.

| ESP32-S3 GPIO | TFT signal | Notes |
| ---: | --- | --- |
| GPIO12 | SCLK / SCK | SPI3 clock. |
| GPIO11 | MOSI / SDI | SPI3 data output. |
| GPIO10 | CS | Dedicated display chip select, active low. |
| GPIO9 | D/C | Data/command select. |
| GPIO8 | RESET | Display reset. |
| GPIO7 | LED / BL | Active-high backlight input driven by ESP32-S3 LEDC PWM at 20 kHz. Brightness is stored as `display_brightness` from 0-100 percent; the default is 80. |
| Not connected | MISO / SDO | Not needed for write-only display operation. |
| 3V3 | VCC / logic supply | Prefer 3.3 V. Check the specific TFT breakout's power and logic-level circuit. |
| GND | GND | Common ground. |

The TFT breakout's microSD socket is currently unused. Firmware storage uses the ESP32-S3's dedicated internal-flash partition.

The module description identifies `LED` as an active-high backlight control input and specifies 3.3 V logic, so GPIO7 can drive its PWM input directly. Power the module from 3.3 V. If the particular board revision exposes the bare backlight LEDs instead of a logic-level input, use a suitable transistor driver and current limiting rather than driving the LEDs from GPIO7. The firmware loads the saved percentage at startup; changing it currently requires saving a new value under the `display_brightness` setting and rebooting because physical settings controls are not connected yet.

### MCP23017 Button Matrix

The expander drives a 5-row by 5-column matrix: ten matrix pins, plus power, I2C and the interrupt. Add one diode per key in series with the switch, as shown below.

#### Solder list

Pin numbers are for the PDIP-28, SOIC-28 and SSOP-28 packages, which share the same numbering; the QFN-28 package is numbered differently, so use the port names there. Most breakout boards silkscreen the port names too, in which case follow the names.

The datasheet names the ports GPA0-GPA7 and GPB0-GPB7; breakouts usually print them as **PA0-PA7** and **PB0-PB7**, which is what the table uses.

| MCP pin | Label | Connect to |
| ---: | --- | --- |
| 9 | VDD | 3V3 (3.3 V logic only) |
| 10 | VSS | GND |
| 15 | A0 | GND |
| 16 | A1 | GND |
| 17 | A2 | GND |
| 18 | RESET | 3V3 through 10 kΩ (or a controlled ESP32 GPIO if reset is needed) |
| 12 | SCL | ESP32-S3 GPIO18, with 4.7 kΩ pull-up to 3V3 |
| 13 | SDA | ESP32-S3 GPIO17, with 4.7 kΩ pull-up to 3V3 |
| 20 | INTA | ESP32-S3 GPIO16; firmware enables the internal pull-up, an external 10 kΩ is optional |
| 19 | INTB | leave open (IOCON mirrors INTB onto INTA) |
| 11, 14 | NC | leave open |
| 21 | PA0 | row 0 |
| 22 | PA1 | row 1 |
| 23 | PA2 | row 2 |
| 24 | PA3 | row 3 |
| 25 | PA4 | column 0 |
| 26 | PA5 | column 1 |
| 27 | PA6 | column 2 |
| 28 | PA7 | column 3 |
| 1 | PB0 | column 4 |
| 2 | PB1 | row 4 |
| 3-8 | PB2-PB7 | leave open (unused pulled-up inputs) |

On a typical breakout one header runs PB0-PB7 and the other runs PA7-PA0, so the two sides are in opposite order. Follow the printed label, not the pin position, and if the board only numbers the pins use the table above.

Strap A0, A1 and A2 low: that is address `0x20`, which is what the firmware uses. Tying any of them high changes the address and the keypad stops answering. If your breakout already ties A0-A2 to GND, do not also strap them elsewhere.

Parts for one keypad: one MCP23017, 24 keys, 24 × 1N4148 (or 1N4148W), 2 × 4.7 kΩ pull-ups for SDA/SCL, and 1 × 10 kΩ for RESET.

#### Key grid and diode orientation

Every cell is the key switch in series with a diode:

```text
row line ────[ key ]────|>|──── column line
                         ^
                         diode band (cathode) faces the column
```

The cell at row `R` and column `C` holds this key:

| row \ column | C0 PA4 | C1 PA5 | C2 PA6 | C3 PA7 | C4 PB0 |
| --- | --- | --- | --- | --- | --- |
| R0 PA0 | `0` | `1` | `2` | `3` | `SEND` |
| R1 PA1 | `4` | `5` | `6` | `7` | `BACK` |
| R2 PA2 | `8` | `9` | `A` | `B` | `ENTER` |
| R3 PA3 | `C` | `D` | `E` | `F` | `HELP` |
| R4 PB1 | `UP` | `DOWN` | `LEFT` | `RIGHT` | unused |

- In the drawing, the bar of `|>|` is the diode cathode and faces the column wire; the plain end (anode) faces the row. Orient the physical part the same way.
- Switch and diode are simply in series, so either order works along the cell: row → switch → diode → column, or row → diode → switch → column. Only the diode direction matters. A convenient build is: one key leg to the row wire, the other key leg to the diode anode, diode cathode (band) to the column wire.
- One diode per key. Through-hole 1N4148 is fine; for a PCB use 1N4148W or a BAT54 in the same orientation.
- The diode is what makes multiple simultaneous keys safe. A scan drives one column low and the other four high, then reads the pulled-up rows. Without the diodes, three keys forming a rectangle connect a driven-high column through two switches to the low column, so the two column outputs fight each other far above the expander's 25 mA per-pin rating; in a floating-column design the same rectangle also produces a phantom fourth key. With the diodes, current can only flow row → key → column.
- Level check: the 100 kΩ row pull-up limits current to about 33 µA, so a silicon diode drops only ~0.3 V, far below the expander's `0.3 x VDD` input-low threshold. Reverse leakage is nanoamps, so unpressed keys cannot pull a row low.
- One cell is unused: row 4 column 4. Do not fit a key there.
- Build tip: bus five row wires horizontally and five column wires vertically, and put each key plus its diode across one row/column crossing. Keep the diode close to the key and check the band direction before soldering all 25.

The matrix is interrupt-driven. Columns idle low, so pressing a key pulls its pulled-up row low and the expander's interrupt-on-change latches INTA low; the worker waits for that falling edge on GPIO16 instead of polling. On a wake it drives one column low while the other four go high, reads the rows, then restores the idle-low state and reads the ports once more to clear the latch. A key must read the same for three 10 ms scans after the edge before it counts as a press, and a burst also runs while a change is still settling, so a bouncing contact cannot lose an event. Idle I2C traffic is zero; each press costs a short burst of scans.

At address `0x20` over a 400 kHz bus, the driver configures the expander as: `IOCON=0x44` (INTB mirrored onto INTA, open-drain active-low output), `IODIRA=0x0F`, `IODIRB=0xFD` (only PB0 is an output), `IPOL=0` (active-low rows), `GPPUA=0x0F`, `GPPUB=0xFE`, `INTCON=0` (interrupt on any change), `GPINTENA=0x0F`, `GPINTENB=0x02` (rows only), and idle `OLATA=0x00` / `OLATB=0x00`. Registers are written one at a time in the reset default BANK=0, byte mode. If the expander does not answer, the driver reconfigures on the next attempt and the UI simply gets no key events.

SEND is the first action key so it sits at the top of the function column, and HELP opens the on-screen legend. Hex digits are typed as ASCII pairs, so `4` then `5` types `E` (`0x45`); the composer shows the pending digit while it waits for its pair.

## Consolidated GPIO Plan

| GPIO | Assignment |
| ---: | --- |
| 0 | Spare; avoid until boot-strapping behavior is reviewed. |
| 1-3 | Spare / ADC-capable. |
| 4 | GPS UART1 RX. |
| 5 | GPS UART1 TX. |
| 6 | Spare. |
| 7 | TFT backlight enable. |
| 8 | TFT reset. |
| 9 | TFT D/C. |
| 10 | TFT CS. |
| 11 | TFT MOSI (SPI3 hardware). |
| 12 | TFT SCLK (SPI3 hardware). |
| 13-15 | Spare. |
| 16 | MCP23017 INTA interrupt (open-drain, active-low) for keypad wake-up. |
| 17 | I2C SDA to MCP23017. |
| 18 | I2C SCL to MCP23017. |
| 38-42, 45-46 | LR1121 signals; do not reassign. |
| 43-44 | UART0; retain for console/programming. |

GPIO27-GPIO32 are not used by the current application. They are not in the supplied 21-pin list, so verify module flash/PSRAM use and board access before assigning them.

## Bring-Up Plan

1. Check the HF-Kit schematic and connector labels against the GPIO numbers above. Confirm that GPIO4-18 are exposed on the connector intended for peripherals and are not reserved by the kit adapter.
2. Verify each breakout's supply voltage, logic levels, pull-ups, and current draw. Power the TFT backlight from a suitable supply, not directly from a GPIO.
3. Verify GPS NMEA output on GPIO4/5 on the target board.
4. Verify ILI9341 updates on SPI3 using GPIO12/11 and its CS/D-C/RESET pins.
5. If adding the MCP23017, bring it up on GPIO17/18, verify address `0x20`, confirm INTA on GPIO16 pulses low on a press and returns high after the scan, then test all five rows and columns through the 5x5 keypad layout. Press three keys that form a rectangle to confirm the per-key diodes prevent a phantom fourth key.
6. Keep LR1121 and GPS GPIO assignments unchanged when wiring the button expander.

## References

- [Waveshare ESP32-S3-LR1121 documentation](https://docs.waveshare.com/ESP32-S3-LR1121-XF)
- [Waveshare hardware resources and HF schematic](https://docs.waveshare.com/ESP32-S3-LR1121-XF/Resources-And-Documents)
- [Waveshare ESP32-S3-LR1121-HF schematic PDF](https://github.com/waveshareteam/ESP32-S3-LR1121-XF/raw/main/hardware/schematics/ESP32-S3-LR1121-HF.pdf)
