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
use core::convert::TryInto;
use panic_halt as _;
use stm32f4xx_hal::stm32::EXTI;
use usb_device::bus::UsbBusAllocator;
use usb_device::prelude::*;

type RcUsbDevice = UsbDevice<'static, UsbBusType>;
type RcUsbClass = HIDClass<'static, UsbBusType>;

const CORE_FREQUENCY_MHZ: u32 = 84;
const REPORT_PERIOD: u32 = 84_000;

#[app(device = stm32f4xx_hal::stm32, peripherals = true, monotonic = rtic::cyccnt::CYCCNT)]
const APP: () = {
    struct Resources {
        //timer: timer::Timer<stm32::TIM3>,
        usb_device: RcUsbDevice,
        usb_class: RcUsbClass,
        ppm_parser: PpmParser,
        ppm_pin: PA0<Input<Floating>>,

        exti: EXTI,
        t0: Instant,
    }

    #[init(schedule=[report])]
    fn init(mut cx: init::Context) -> init::LateResources {
        static mut EP_MEMORY: [u32; 1024] = [0; 1024];
        static mut USB_BUS: Option<UsbBusAllocator<UsbBusType>> = None;

        //Enable Time Measurement
        cx.core.DCB.enable_trace();
        cx.core.DWT.enable_cycle_counter();

        let rcc = cx.device.RCC.constrain();
        let gpioa = cx.device.GPIOA.split();
        let _gpiob = cx.device.GPIOB.split();
        let _gpioc = cx.device.GPIOC.split();

        let _clocks = rcc
            .cfgr
            .use_hse(25.mhz())
            .sysclk(CORE_FREQUENCY_MHZ.mhz())
            .require_pll48clk()
            .freeze();

        //// USB initialization
        let usb = USB {
            usb_global: cx.device.OTG_FS_GLOBAL,
            usb_device: cx.device.OTG_FS_DEVICE,
            usb_pwrclk: cx.device.OTG_FS_PWRCLK,
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
            .product("RC USB Controller")
            .serial_number(env!("CARGO_PKG_VERSION"))
            .build();

        //Initialize Interrupt Input
        let mut syscfg = cx.device.SYSCFG;
        let mut exti = cx.device.EXTI;

        let mut ppm_pin = gpioa.pa0.into_floating_input();
        ppm_pin.make_interrupt_source(&mut syscfg);
        ppm_pin.enable_interrupt(&mut exti);
        ppm_pin.trigger_on_edge(&mut exti, Edge::FALLING);

        let mut ppm_parser = PpmParser::new();
        ppm_parser.set_channel_limits(500, 1500);
        ppm_parser.set_minimum_channels(12);
        ppm_parser.set_sync_width(4000);
        ppm_parser.set_max_ppm_time(core::u32::MAX / CORE_FREQUENCY_MHZ);

        // enqueu
        cx.schedule
            .report(cx.start + REPORT_PERIOD.cycles())
            .unwrap();

        init::LateResources {
            usb_device,
            usb_class,
            exti,
            ppm_pin,
            ppm_parser,
            t0: cx.start,
        }
    }

    #[task(binds = EXTI0, resources = [ppm_parser, ppm_pin, t0], priority = 3)]
    fn ppm_falling(cx: ppm_falling::Context) {
        cx.resources.ppm_pin.clear_interrupt_pending_bit();

        let ppm_parser: &mut PpmParser = cx.resources.ppm_parser;
        let cycles_since_start = cx.resources.t0.elapsed().as_cycles();
        ppm_parser.handle_pulse_start(cycles_since_start / CORE_FREQUENCY_MHZ)
    }

    // Periodic status update to Computer (every millisecond)
    #[task(resources = [usb_class, ppm_parser], schedule=[report], priority = 1)]
    fn report(mut cx: report::Context) {
        // schedule itself to keep the loop running
        cx.schedule
            .report(cx.scheduled + REPORT_PERIOD.cycles())
            .unwrap();

        let mut frame: Option<PpmFrame> = None;

        cx.resources.ppm_parser.lock(|parser: &mut PpmParser| {
            frame = parser.next_frame();
        });

        //TODO commented out
        //let report = match frame {
        //    None => return,
        //    Some(frame) => frame.chan_values[0..12].try_into().unwrap(),
        //};

        //Lock usb_class object and report
        //cx.resources
        //    .usb_class
        //    .lock(|class| class.write(&get_report(&report)));

        //TODO Zum Testen
        cx.resources.usb_class.lock(|class| {
            class.write(&get_report(&[1400; 16]));
        });
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

fn usb_poll(usb_device: &mut RcUsbDevice, ppm_device: &mut RcUsbClass) {
    usb_device.poll(&mut [ppm_device]);
}
