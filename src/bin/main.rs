#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]
#![deny(clippy::large_stack_frames)]

use core::sync::atomic::{AtomicU16, Ordering};

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

#[allow(
    clippy::large_stack_frames,
    reason = "it's not unusual to allocate larger buffers etc. in main"
)]
static VOLTAGE: AtomicU16 = AtomicU16::new(0);
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
            const TEXT_HEIGHT: i32 = 20;
            const VERTICAL_POSITION: i32 = (DISPLAY_HEIGHT - TEXT_HEIGHT) / 2;
            let text_style = MonoTextStyleBuilder::new()
                .font(&FONT_10X20)
                .text_color(BinaryColor::On)
                .build();
            loop {
                let delay_start = Instant::now();
                display.clear_buffer();

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
                info!("Text delay: {:?}", &delay_start.elapsed());
            }
        },
    );

    let analog_pin = peripherals.GPIO19;
    let mut adc1_config = AdcConfig::<ADC2>::new();
    let mut pin =
        adc1_config.enable_pin_with_cal::<_, AdcCalLine<ADC2>>(analog_pin, Attenuation::_11dB);
    let mut adc1 = Adc::new(peripherals.ADC2, adc1_config);

    loop {
        let pin_delay_start = Instant::now();
        const N: u64 = 1024;
        let pin_values: u64 = (0..N).map(|_| adc1.read_blocking(&mut pin) as u64).sum();
        let new_voltage = (pin_values * 3300 / N / 3629) as u16;
        VOLTAGE.store(new_voltage, Ordering::Relaxed);
        info!("{} mV {:?}", new_voltage, pin_delay_start.elapsed());
    }

    // for inspiration have a look at the examples at https://github.com/esp-rs/esp-hal/tree/esp-hal-v1.2.2/examples
}
