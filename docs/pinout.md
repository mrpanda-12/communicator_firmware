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

## Proposed External Peripherals

### u-blox NEO-M6N GPS

Use UART1, leaving UART0 available for programming and logs.

| ESP32-S3 | GPS module | Notes |
| --- | --- | --- |
| GPIO4 / UART1 RX | GPS TX | Cross TX to RX. |
| GPIO5 / UART1 TX | GPS RX | Cross RX to TX. |
| GPIO6 | GPS PPS (optional) | Leave disconnected until PPS timestamps are needed. |
| 3V3 or module-rated supply | VCC | Confirm the exact NEO-M6N breakout's input range and current before selecting the rail. |
| GND | GND | Common ground. |

Start with the module's documented baud rate and 8-N-1 framing; do not assume the receiver is still at its factory baud if it has been configured before. Confirm UART RX/TX with a serial monitor before adding PPS handling.

The firmware wrapper is [`hardware/gps.rs`](../src/bin/hardware/gps.rs). It defaults to 9600 baud and uses the `nmea` crate in `no_std` mode with GGA/RMC parsing enabled. Poll it from the application loop or a periodic Embassy task; `poll()` drains only already-buffered UART bytes and does not wait for input:

```rust
let mut gps = Gps::new(peripherals.UART1, peripherals.GPIO4, peripherals.GPIO5)?;

loop {
	gps.poll()?;
	if let Some(coordinate) = gps.get_coordinate() {
		// coordinate.latitude_degrees and coordinate.longitude_degrees
	}
	if let Some(time) = gps.get_time() {
		// UTC date (when RMC has supplied one) and time-of-day
	}
}
```

`get_coordinate()` and `get_time()` return the most recently parsed valid data as `Option`; malformed/checksum-invalid sentences are ignored and exposed through `last_sentence_error()`. The PPS pin is only reserved in the pin map; timestamp capture is not implemented.

### ILI9341 2.4-Inch SPI TFT

Use a separate SPI controller from the LR1121. `SPI3` is the proposed ESP-HAL peripheral; configure pins through the ESP32-S3 GPIO matrix after checking the selected HAL version and exposed headers.

| ESP32-S3 GPIO | TFT signal | Notes |
| ---: | --- | --- |
| GPIO12 | SCLK / SCK | SPI clock. |
| GPIO11 | MOSI / SDI | Display data input. |
| GPIO10 | CS | Dedicated display chip select, active low. |
| GPIO9 | D/C | Data/command select. |
| GPIO8 | RESET | Display reset. |
| GPIO7 | LED / BL | Backlight enable; use PWM only if the breakout supports it and current is within its rating. |
| Not connected | MISO / SDO | Not needed for write-only display operation. |
| 3V3 | VCC / logic supply | Prefer 3.3 V. Check the specific TFT breakout's power and logic-level circuit. |
| GND | GND | Common ground. |

The TFT's microSD socket is intentionally left unused in this plan. If enabled later, share SCLK/MOSI/MISO with the display but assign a separate SD chip-select and ensure both chip-selects remain inactive while the other device is used. GPIO6 is currently left available and could be considered for SD CS instead of GPS PPS.

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
| 6 | GPS PPS optional; reserve for SD CS if microSD is added instead. |
| 7 | TFT backlight enable. |
| 8 | TFT reset. |
| 9 | TFT D/C. |
| 10 | TFT CS. |
| 11 | TFT MOSI. |
| 12 | TFT SCLK. |
| 13-15 | Spare / ADC-capable. |
| 16 | MCP23017 interrupt. |
| 17 | I2C SDA to MCP23017. |
| 18 | I2C SCL to MCP23017. |
| 38-42, 45-46 | LR1121 signals; do not reassign. |
| 43-44 | UART0; retain for console/programming. |

GPIO27-GPIO32 are claimed in `main.rs` but are not in the supplied 21-pin list. Do not rely on them as accessible expansion GPIOs; verify their module flash/PSRAM use before changing those claims.

## Bring-Up Plan

1. Check the HF-Kit schematic and connector labels against the GPIO numbers above. Confirm that GPIO4-18 are exposed on the connector intended for peripherals and are not reserved by the kit adapter.
2. Verify each breakout's supply voltage, logic levels, pull-ups, and current draw. Power the TFT backlight from a suitable supply, not directly from a GPIO.
3. Bring up GPS UART alone and verify NMEA output on GPIO4/5. Add GPIO6 PPS only after UART is stable.
4. Bring up the ILI9341 on SPI3 using GPIO12/11 and its CS/D-C/RESET pins. Start at a conservative SPI clock and increase only after reliable display updates.
5. Bring up MCP23017 on GPIO17/18, verify address `0x20`, then test rows/columns and the interrupt on GPIO16.
6. Add each driver to `main.rs` only after its isolated electrical and bus test passes; do not change the existing LR1121 GPIO assignments.

## References

- [Waveshare ESP32-S3-LR1121 documentation](https://docs.waveshare.com/ESP32-S3-LR1121-XF)
- [Waveshare hardware resources and HF schematic](https://docs.waveshare.com/ESP32-S3-LR1121-XF/Resources-And-Documents)
- [Waveshare ESP32-S3-LR1121-HF schematic PDF](https://github.com/waveshareteam/ESP32-S3-LR1121-XF/raw/main/hardware/schematics/ESP32-S3-LR1121-HF.pdf)