use chrono::{Datelike, NaiveDate, NaiveTime, Timelike};
use esp_hal::{
    peripherals::{GPIO4, GPIO5, UART1},
    uart::{Config, ConfigError, RxError, Uart},
};
use nmea::{ParseResult, SENTENCE_MAX_LEN, parse_bytes, sentences::rmc::RmcStatusOfFix};

const DEFAULT_BAUD_RATE: u32 = 9_600;
const UART_READ_BUFFER_SIZE: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Coordinate {
    pub latitude_degrees: f64,
    pub longitude_degrees: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GpsDate {
    pub year: i32,
    pub month: u32,
    pub day: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GpsTime {
    pub date: Option<GpsDate>,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
    pub nanosecond: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SentenceError {
    TooLong,
    InvalidNmea,
}

pub struct Gps<'d> {
    uart: Uart<'d, esp_hal::Blocking>,
    sentence: [u8; SENTENCE_MAX_LEN],
    sentence_len: usize,
    collecting_sentence: bool,
    pub coordinate: Option<Coordinate>,
    pub time: Option<GpsTime>,
    pub last_sentence_error: Option<SentenceError>,
}

impl<'d> Gps<'d> {
    // Initializes UART1 on the board's proposed GPS RX/TX pins at 9600 baud.
    pub fn new(uart: UART1<'d>, rx: GPIO4<'d>, tx: GPIO5<'d>) -> Result<Self, ConfigError> {
        Self::new_with_baud_rate(uart, rx, tx, DEFAULT_BAUD_RATE)
    }

    pub fn new_with_baud_rate(
        uart: UART1<'d>,
        rx: GPIO4<'d>,
        tx: GPIO5<'d>,
        baud_rate: u32,
    ) -> Result<Self, ConfigError> {
        let uart = Uart::new(uart, Config::default().with_baudrate(baud_rate))?
            .with_rx(rx)
            .with_tx(tx);

        Ok(Self {
            uart,
            sentence: [0; SENTENCE_MAX_LEN],
            sentence_len: 0,
            collecting_sentence: false,
            coordinate: None,
            time: None,
            last_sentence_error: None,
        })
    }

    // Consumes currently buffered UART bytes without waiting for new input.
    // Returns the number of bytes read; parsed sentences update the cached fix.
    pub fn poll(&mut self) -> Result<usize, RxError> {
        let mut total = 0;
        let mut bytes = [0; UART_READ_BUFFER_SIZE];

        loop {
            let count = self.uart.read_buffered(&mut bytes)?;
            if count == 0 {
                break;
            }

            total += count;
            for byte in &bytes[..count] {
                self.feed_byte(*byte);
            }
        }

        Ok(total)
    }

    pub fn get_coordinate(&self) -> Option<Coordinate> {
        self.coordinate
    }

    pub fn has_fix(&self) -> bool {
        self.coordinate.is_some()
    }

    // Returns the latest UTC time, with date populated when an RMC sentence arrived.
    pub fn get_time(&self) -> Option<GpsTime> {
        self.time
    }

    pub fn last_sentence_error(&self) -> Option<SentenceError> {
        self.last_sentence_error
    }

    // Drains buffered UART bytes and updates coordinates/time.
    pub fn update(&mut self) -> Result<usize, RxError> {
        self.poll()
    }

    fn feed_byte(&mut self, byte: u8) {
        if byte == b'$' {
            self.sentence_len = 0;
            self.collecting_sentence = true;
        } else if !self.collecting_sentence {
            return;
        }

        if byte == b'\n' {
            if self.sentence_len > 0 && self.sentence[self.sentence_len - 1] == b'\r' {
                self.sentence_len -= 1;
            }
            self.parse_current_sentence();
            self.collecting_sentence = false;
            self.sentence_len = 0;
            return;
        }

        if self.sentence_len == self.sentence.len() {
            self.last_sentence_error = Some(SentenceError::TooLong);
            self.collecting_sentence = false;
            self.sentence_len = 0;
            return;
        }

        self.sentence[self.sentence_len] = byte;
        self.sentence_len += 1;
    }

    fn parse_current_sentence(&mut self) {
        match parse_bytes(&self.sentence[..self.sentence_len]) {
            Ok(ParseResult::GGA(gga)) => {
                if gga.fix_type.is_some_and(|fix_type| fix_type.is_valid()) {
                    self.coordinate = coordinate_from(gga.latitude, gga.longitude);
                } else {
                    self.coordinate = None;
                }

                if let Some(fix_time) = gga.fix_time {
                    self.update_time(fix_time, None);
                }
                self.last_sentence_error = None;
            }
            Ok(ParseResult::RMC(rmc)) => {
                if rmc.status_of_fix == RmcStatusOfFix::Invalid {
                    self.coordinate = None;
                } else {
                    self.coordinate = coordinate_from(rmc.lat, rmc.lon);
                }

                if let Some(fix_time) = rmc.fix_time {
                    self.update_time(fix_time, rmc.fix_date);
                }
                self.last_sentence_error = None;
            }
            Ok(_) => self.last_sentence_error = None,
            Err(_) => self.last_sentence_error = Some(SentenceError::InvalidNmea),
        }
    }

    fn update_time(&mut self, time: NaiveTime, date: Option<NaiveDate>) {
        let date = date
            .map(|date| GpsDate {
                year: date.year(),
                month: date.month(),
                day: date.day(),
            })
            .or_else(|| self.time.and_then(|time| time.date));

        self.time = Some(GpsTime {
            date,
            hour: time.hour(),
            minute: time.minute(),
            second: time.second(),
            nanosecond: time.nanosecond(),
        });
    }
}

fn coordinate_from(latitude: Option<f64>, longitude: Option<f64>) -> Option<Coordinate> {
    Some(Coordinate {
        latitude_degrees: latitude?,
        longitude_degrees: longitude?,
    })
}
