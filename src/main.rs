#![deny(unsafe_code)]
#![no_main]
#![no_std]

mod hid;

use hid::*;
use ppm_decode::{PpmFrame, PpmParser};
pub use rtic::{
    app,
    cyccnt::{Instant, U32Ext},
};
use stm32f4xx_hal::gpio::gpioa::PA0;
use stm32f4xx_hal::gpio::{Edge, ExtiPin, Floating, Input};
use stm32f4xx_hal::otg_fs::{UsbBusType, USB};
use stm32f4xx_hal::prelude::*;
use stm32f4xx_hal::stm32;
use stm32f4xx_hal::stm32::EXTI;
use stm32f4xx_hal::timer;
use usb_device::bus::UsbBusAllocator;
use usb_device::prelude::*;
use core::convert::TryInto;
use panic_halt as _;

type UsbPpmDevice = UsbDevice<'static, UsbBusType>;
type UsbPpmClass = HIDClass<'static, UsbBusType>;

pub const CORE_FREQUENCY_MHZ: u32 = 84;

#[app(device = stm32f4xx_hal::stm32, peripherals = true, monotonic = rtic::cyccnt::CYCCNT)]
const APP: () = {
    struct Resources {
        timer: timer::Timer<stm32::TIM3>,
        usb_device: UsbPpmDevice,
        usb_class: UsbPpmClass,
        ppm_parser: PpmParser,
        ppm_pin: PA0<Input<Floating>>,
        exti: EXTI,
    }

    #[init]
    fn init(mut c: init::Context) -> init::LateResources {
        static mut EP_MEMORY: [u32; 1024] = [0; 1024];
        static mut USB_BUS: Option<UsbBusAllocator<UsbBusType>> = None;

        //Enable Time Measurement
        c.core.DWT.enable_cycle_counter();
        c.core.DCB.enable_trace();

        let rcc = c.device.RCC.constrain();
        let gpioa = c.device.GPIOA.split();
        let _gpiob = c.device.GPIOB.split();
        let _gpioc = c.device.GPIOC.split();

        let clocks = rcc
            .cfgr
            .use_hse(25.mhz())
            .sysclk(CORE_FREQUENCY_MHZ.mhz())
            .require_pll48clk()
            .freeze();

        //// USB initialization
        let usb = USB {
            usb_global: c.device.OTG_FS_GLOBAL,
            usb_device: c.device.OTG_FS_DEVICE,
            usb_pwrclk: c.device.OTG_FS_PWRCLK,
            pin_dm: gpioa.pa11.into_alternate_af10(),
            pin_dp: gpioa.pa12.into_alternate_af10(),
        };

        *USB_BUS = Some(UsbBusType::new(usb, EP_MEMORY));
        let usb_bus = USB_BUS.as_ref().unwrap();

        let usb_class = HIDClass::new(usb_bus);
        // https://github.com/obdev/v-usb/blob/master/usbdrv/USB-IDs-for-free.txt
        // For USB Joystick as there is no USB Game Pad on this free ID list
        let usb_device = UsbDeviceBuilder::new(usb_bus, UsbVidPid(0x16c0, 0x27dc))
            .manufacturer("autumnal.de")
            .product("PpmController")
            .serial_number(env!("CARGO_PKG_VERSION"))
            .build();

        //Initialize Interrupt Input
        let mut syscfg = c.device.SYSCFG;
        let mut exti = c.device.EXTI;

        // Use timer to trigger TIM3 Interrupt every milli second
        let mut timer = timer::Timer::tim3(c.device.TIM3, 1.khz(), clocks);
        timer.listen(timer::Event::TimeOut);

        let mut ppm_pin = gpioa.pa0.into_floating_input();
        ppm_pin.make_interrupt_source(&mut syscfg);
        ppm_pin.enable_interrupt(&mut exti);
        ppm_pin.trigger_on_edge(&mut exti, Edge::FALLING);

        let mut ppm_parser = PpmParser::new();
        ppm_parser.set_channel_limits(500, 1500);
        ppm_parser.set_minimum_channels(12);
        ppm_parser.set_sync_width(4000);

        init::LateResources {
            timer,
            usb_device,
            usb_class,
            exti,
            ppm_pin,
            ppm_parser,
        }
    }

    #[task(binds = EXTI0, resources = [ppm_parser, ppm_pin], priority = 3)]
    fn ppm_rise(c: ppm_rise::Context) {
        c.resources.ppm_pin.clear_interrupt_pending_bit();

        let ppm_parser: &mut PpmParser = c.resources.ppm_parser;

        ppm_parser.handle_pulse_start(Instant::now().elapsed().as_cycles() / CORE_FREQUENCY_MHZ)
    }

    // Periodic status update to Computer (every millisecond)
    #[task(binds = TIM3, resources = [usb_class, timer, ppm_parser], priority = 1)]
    fn report(mut c: report::Context) {
        c.resources.timer.clear_interrupt(timer::Event::TimeOut);
        let mut frame: Option<PpmFrame> = None;

        c.resources.ppm_parser.lock(|parser: &mut PpmParser| {
            frame = parser.next_frame();
        });

        let report = match frame{
            None => return,
            Some(frame) => frame.chan_values[0..12].try_into().unwrap(),
        };

        //Lock usb_class object and report
        c.resources.usb_class.lock(|class| class.write(&get_report(&report)));

        //TODO Zum Testen
        //c.resources
        //    .usb_class
        //    .lock(|class| {
        //        class.write(&get_report(&[1500; 16]));
        //    });
    }

    // Global USB Interrupt (does not include Wakeup)
    #[task(binds = OTG_FS, resources = [usb_device, usb_class], priority = 2)]
    fn usb_tx(mut c: usb_tx::Context) {
        usb_poll(&mut c.resources.usb_device, &mut c.resources.usb_class);
    }

    // Interrupt for USB Wakeup
    #[task(binds = OTG_FS_WKUP, resources = [usb_device, usb_class], priority = 2)]
    fn usb_rx(mut c: usb_rx::Context) {
        usb_poll(&mut c.resources.usb_device, &mut c.resources.usb_class);
    }

    extern "C" {
        //Any free Interrupt which is used for the Debounce Software Task
        fn SDIO();
    }
};

fn usb_poll(usb_device: &mut UsbPpmDevice, ppm_device: &mut UsbPpmClass) {
    usb_device.poll(&mut [ppm_device]);
}
