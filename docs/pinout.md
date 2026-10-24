# Pinout Plan

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

The GPS and TFT wiring below is configured in the current firmware. MCP23017 wiring is still a plan; there is no I2C button driver or physical button input task yet.

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
| GPIO7 | LED / BL | Backlight enable; use PWM only if the breakout supports it and current is within its rating. |
| Not connected | MISO / SDO | Not needed for write-only display operation. |
| 3V3 | VCC / logic supply | Prefer 3.3 V. Check the specific TFT breakout's power and logic-level circuit. |
| GND | GND | Common ground. |

The TFT breakout's microSD socket is currently unused. Firmware storage uses the ESP32-S3's dedicated internal-flash partition.

### MCP23017 Button Matrix

Connect the expander to I2C and use its 16 GPIO pins for a 4-row by 5-column matrix (nine expander pins total).

| ESP32-S3 GPIO | MCP23017 | Notes |
| ---: | --- | --- |
| GPIO17 | SDA | I2C data. |
| GPIO18 | SCL | I2C clock. |
| GPIO16 | INTA or INTB | Active-low interrupt output; configure open-drain/polarity consistently with the pull-up. |
| 3V3 | VDD | Use 3.3 V logic. |
| GND | VSS | Common ground. |
| 3V3 | A0, A1, A2 | Example address strap for address `0x20`; change only if the bus has another device at that address. |
| GND | RESET | Hold reset high, or connect to a controlled reset GPIO if required by the breakout. |

Suggested matrix allocation:

| MCP23017 pins | Matrix role |
| --- | --- |
| GPA0-GPA3 | Row inputs with pull-ups |
| GPA4-GPA7 | Column drive outputs |
| GPB0 | Fifth column drive output |
| GPB1-GPB7 | Spare |

Firmware can idle all columns as low outputs so a pressed key pulls a row low and triggers the MCP23017 interrupt. For scanning, change all columns to inputs, then make one column a low output at a time and sample the four pulled-up rows; restore the idle state afterward. Configure the interrupt polarity/open-drain option and its pull-up according to the MCP23017 breakout and datasheet. If simultaneous multi-key rollover is required, add a diode per key; otherwise a 4×5 matrix can ghost when multiple keys are pressed.

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
| 16 | MCP23017 interrupt. |
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
5. If adding the MCP23017, bring it up on GPIO17/18, verify address `0x20`, then test rows/columns and the interrupt on GPIO16.
6. Keep LR1121 and GPS GPIO assignments unchanged when wiring the button expander.

## References

- [Waveshare ESP32-S3-LR1121 documentation](https://docs.waveshare.com/ESP32-S3-LR1121-XF)
- [Waveshare hardware resources and HF schematic](https://docs.waveshare.com/ESP32-S3-LR1121-XF/Resources-And-Documents)
- [Waveshare ESP32-S3-LR1121-HF schematic PDF](https://github.com/waveshareteam/ESP32-S3-LR1121-XF/raw/main/hardware/schematics/ESP32-S3-LR1121-HF.pdf)
