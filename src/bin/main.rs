#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]
#![deny(clippy::large_stack_frames)]

use esp_hal::analog::adc::{Adc, AdcCalLine, AdcConfig, Attenuation};
use esp_hal::peripherals::ADC2;
use esp_hal::system::Stack;
use esp_hal::timer::timg::TimerGroup;

use defmt::info;
use esp_backtrace as _;
use esp_hal::clock::CpuClock;
use esp_hal::main;
use esp_hal::time::{Duration, Instant};
use esp_println as _;
use static_cell::ConstStaticCell;

// This creates a default app-descriptor required by the esp-idf bootloader.
// For more information see: <https://docs.espressif.com/projects/esp-idf/en/stable/esp32/api-reference/system/app_image_format.html#application-description>
esp_bootloader_esp_idf::esp_app_desc!();

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
        || {
            loop {
                info!("Hello world from the second core!");
                let delay_start = Instant::now();
                while delay_start.elapsed() < Duration::from_millis(500) {}
            }
        },
    );

    let analog_pin = peripherals.GPIO19;
    let mut adc1_config = AdcConfig::<ADC2>::new();
    let mut pin =
        adc1_config.enable_pin_with_cal::<_, AdcCalLine<ADC2>>(analog_pin, Attenuation::_11dB);
    let mut adc1 = Adc::new(peripherals.ADC2, adc1_config);

    loop {
        let delay_start = Instant::now();

        let pin_value = adc1.read_blocking(&mut pin);
        info!("{} mV", pin_value as u32 * 3300 / 3629);

        while delay_start.elapsed() < Duration::from_millis(5) {}
    }

    // for inspiration have a look at the examples at https://github.com/esp-rs/esp-hal/tree/esp-hal-v1.2.2/examples
}
