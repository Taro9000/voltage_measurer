#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]
#![deny(clippy::large_stack_frames)]

use core::sync::atomic::{AtomicU8, AtomicU16, Ordering};

use embedded_graphics::mono_font::jis_x0201::FONT_10X20;
use esp_hal::analog::adc::{Adc, AdcCalLine, AdcConfig, Attenuation};
use esp_hal::gpio::{Level, Output, OutputConfig};
use esp_hal::peripherals::ADC2;
use esp_hal::system::Stack;
use esp_hal::timer::timg::TimerGroup;

use defmt::{error, info};
use esp_backtrace as _;
use esp_hal::clock::CpuClock;
use esp_hal::main;
use esp_hal::time::{Duration, Instant};
use esp_hal::uart::Config as UartConfig;
use esp_hal::uart::Uart;
use esp_println as _;
use static_cell::ConstStaticCell;

// I2C
use esp_hal::i2c::master::I2c;
use esp_hal::i2c::master::{ClockSource, Config as I2cConfig}; // for convenience, importing as alias
use esp_hal::time::Rate;

// OLED
use ssd1306::{I2CDisplayInterface, Ssd1306, prelude::*};

// Embedded Graphics
use embedded_graphics::{
    mono_font::MonoTextStyleBuilder,
    pixelcolor::BinaryColor,
    prelude::Point,
    prelude::*,
    text::{Baseline, Text},
};

// This creates a default app-descriptor required by the esp-idf bootloader.
// For more information see: <https://docs.espressif.com/projects/esp-idf/en/stable/esp32/api-reference/system/app_image_format.html#application-description>
esp_bootloader_esp_idf::esp_app_desc!();

static VOLTAGE: AtomicU16 = AtomicU16::new(0);
/// 0. Disconnected.
/// 1. Connecting...
/// 2. Connected!
static CONNECTED: AtomicU8 = AtomicU8::new(0);

#[allow(
    clippy::large_stack_frames,
    reason = "it's not unusual to allocate larger buffers etc. in main"
)]
#[main]
fn main() -> ! {
    // generator version: 2.0.0
    // generator parameters: -o esp32s3 -o defmt -o esp-backtrace -o zed

    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);
    let timg0 = TimerGroup::new(peripherals.TIMG0);
    esp_rtos::start(timg0.timer0, peripherals.FROM_CPU_INTR0);
    static STACK: ConstStaticCell<Stack<8192>> = ConstStaticCell::new(Stack::new());

    esp_rtos::start_second_core(
        peripherals.CPU_CTRL,
        peripherals.FROM_CPU_INTR1,
        STACK.take(),
        #[allow(
            clippy::large_stack_frames,
            reason = "it's not unusual to allocate larger buffers etc. in the second core"
        )]
        || {
            let _vext_ctrl = Output::new(
                peripherals.GPIO36,
                Level::Low, // LOW enables the 3.3V Vext rail
                OutputConfig::default(),
            );
            let delay_start = Instant::now();
            while delay_start.elapsed() < Duration::from_millis(50) {}
            let mut oled_reset = Output::new(
                peripherals.GPIO21, // Correct reset pin for V3.2
                Level::Low,         // Start with reset asserted
                OutputConfig::default(),
            );

            let delay_start = Instant::now();
            while delay_start.elapsed() < Duration::from_millis(10) {}
            oled_reset.set_high();
            let delay_start = Instant::now();
            while delay_start.elapsed() < Duration::from_millis(10) {}

            let i2c_bus = I2c::new(
                peripherals.I2C1,
                // I2cConfig is alias of esp_hal::i2c::master::I2c::Config
                I2cConfig::default()
                    .with_clock_source(ClockSource::Xtal)
                    .with_frequency(Rate::from_khz(2750)),
            )
            .unwrap()
            .with_scl(peripherals.GPIO18) // 18
            .with_sda(peripherals.GPIO17); // 17
            let interface = I2CDisplayInterface::new(i2c_bus);
            // initialize the display
            let mut display = Ssd1306::new(interface, DisplaySize128x64, DisplayRotation::Rotate0)
                .into_buffered_graphics_mode();
            match display.init() {
                Ok(_) => {}
                Err(error) => {
                    error!("{:?}", error);
                    panic!("")
                }
            }
            const DISPLAY_WIDTH: i32 = 128;
            const DISPLAY_HEIGHT: i32 = 64;
            const TEXT_WIDTH: i32 = 10;
            const _TEXT_HEIGHT: i32 = 20;
            const VERTICAL_POSITION: i32 = DISPLAY_HEIGHT / 2;
            let text_style = MonoTextStyleBuilder::new()
                .font(&FONT_10X20)
                .text_color(BinaryColor::On)
                .build();
            loop {
                let delay_start = Instant::now();
                display.clear_buffer();
                Text::with_baseline(
                    match CONNECTED.load(Ordering::Relaxed) {
                        0 => "Disconnected.",
                        1 => "Connecting...",
                        2 => "Connected!",
                        _ => "What?!",
                    },
                    Point::new(0, 0),
                    text_style,
                    Baseline::Top,
                )
                .draw(&mut display)
                .unwrap();

                let mut x = DISPLAY_WIDTH - (TEXT_WIDTH * 2);
                Text::with_baseline(
                    "mV",
                    Point::new(x, VERTICAL_POSITION),
                    text_style,
                    Baseline::Top,
                )
                .draw(&mut display)
                .unwrap();
                x -= TEXT_WIDTH;

                let mut voltage = VOLTAGE.load(Ordering::Relaxed);
                if voltage == 0 {
                    x -= TEXT_WIDTH;
                    Text::with_baseline(
                        "0",
                        Point::new(x, VERTICAL_POSITION),
                        text_style,
                        Baseline::Top,
                    )
                    .draw(&mut display)
                    .unwrap();
                } else {
                    while voltage > 0 {
                        x -= TEXT_WIDTH;
                        Text::with_baseline(
                            match voltage % 10 {
                                9 => "9",
                                8 => "8",
                                7 => "7",
                                6 => "6",
                                5 => "5",
                                4 => "4",
                                3 => "3",
                                2 => "2",
                                1 => "1",
                                0 => "0",
                                _ => "?",
                            },
                            Point::new(x, VERTICAL_POSITION),
                            text_style,
                            Baseline::Top,
                        )
                        .draw(&mut display)
                        .unwrap();
                        voltage /= 10;
                    }
                }
                display.flush().unwrap();
                // info!("Text delay: {:?}", &delay_start.elapsed());
            }
        },
    );

    let (mut rx, mut tx) = Uart::new(peripherals.UART0, UartConfig::default())
        .unwrap()
        .with_rx(peripherals.GPIO44) // ✅ Connect RX to GPIO44
        .with_tx(peripherals.GPIO43)
        .split();
    let mut buf = [0u8; 187];
    let mut buf_byte = [0u8; 1];
    let mut buf_change_index = 0;

    let analog_pin = peripherals.GPIO19;
    let mut adc1_config = AdcConfig::<ADC2>::new();
    let mut pin =
        adc1_config.enable_pin_with_cal::<_, AdcCalLine<ADC2>>(analog_pin, Attenuation::_11dB);
    let mut adc1 = Adc::new(peripherals.ADC2, adc1_config);

    const N: u64 = 1; // 1024;
    loop {
        let pin_delay_start = Instant::now();
        let read_voltage = || adc1.read_blocking(&mut pin);
        match CONNECTED.load(Ordering::Relaxed) {
            0 => {
                let new_voltage = average_adc_readings_over_n_times::<1024>(read_voltage);
                VOLTAGE.store(new_voltage, Ordering::Relaxed);
                if !rx.read_ready() {
                    CONNECTED.store(1, Ordering::Relaxed);
                }
            }
            1 => {
                let new_voltage = average_adc_readings_over_n_times::<1>(read_voltage);
                VOLTAGE.store(new_voltage, Ordering::Relaxed);
                let Ok(length) = rx.read_buffered(&mut buf_byte) else {
                    CONNECTED.store(0, Ordering::Relaxed);
                    continue;
                };
                if length == 0 {
                    continue;
                }
                let buf_byte = buf_byte[0];
                if buf_byte != b'\n' && buf_change_index < buf.len() {
                    buf[buf_change_index] = buf_byte;
                    buf_change_index += 1;
                    continue;
                }
                let msg = &buf[..buf_change_index];
                buf_change_index = 0;

                if msg == b"Umm, h-hi~. \
                    Is this perhaps by any chance a Capacitive Soil Moisture Sensor Module Esp32 relayed \
                    that just so happens to be made by Taro900?. \
                    No offense if not my d-dear slave~. *blushes*" {
                    let Ok(_) = tx.write(b"Indeed, master.\n") else {
                        CONNECTED.store(0, Ordering::Relaxed);continue };
                    let Ok(_) = tx.flush() else {
                        CONNECTED.store(0, Ordering::Relaxed);continue };
                    CONNECTED.store(2, Ordering::Relaxed);
                } else {
                    let _ = tx.write(b"Got: ");
                    let _ = tx.write(msg) ;
                    let _ = tx.write(b"\n");
                    let _ = tx.flush();
                }
            }
            2 => {
                let new_voltage = average_adc_readings_over_n_times::<1024>(read_voltage);
                VOLTAGE.store(new_voltage, Ordering::Relaxed);
                let [first, last] = new_voltage.to_ne_bytes();
                let Ok(_) = tx.write(b"\t") else {
                    CONNECTED.store(0, Ordering::Relaxed);
                    continue;
                };
                let Ok(_) = tx.write(&[first]) else {
                    CONNECTED.store(0, Ordering::Relaxed);
                    continue;
                };
                let Ok(_) = tx.write(b"\t") else {
                    CONNECTED.store(0, Ordering::Relaxed);
                    continue;
                };
                let Ok(_) = tx.write(&[last]) else {
                    CONNECTED.store(0, Ordering::Relaxed);
                    continue;
                };
                let Ok(_) = tx.write(b"\n") else {
                    CONNECTED.store(0, Ordering::Relaxed);
                    continue;
                };
            }
            _ => {
                CONNECTED.store(1, Ordering::Relaxed);
            }
        }
        // info!("{} mV {:?}", new_voltage, pin_delay_start.elapsed());
    }

    // for inspiration have a look at the examples at https://github.com/esp-rs/esp-hal/tree/esp-hal-v1.2.2/examples
}

fn average_adc_readings_over_n_times<const N: u64>(mut f: impl FnMut() -> u16) -> u16 {
    let pin_values: u64 = (0..N).map(|_| f() as u64).sum();
    (pin_values * 3300 / N / 3629) as u16
}
