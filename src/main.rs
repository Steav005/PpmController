#![deny(unsafe_code)]
#![no_main]
#![no_std]

mod hid;

use core::convert::TryInto;
use hid::*;
use panic_halt as _;
use ppm_decode::{PpmFrame, PpmParser};
pub use rtic::{
    app,
    cyccnt::{Instant, U32Ext},
};
use stm32f4xx_hal::gpio::{gpioa::PA0, gpioc::PC13};
use stm32f4xx_hal::gpio::{Edge, ExtiPin, Floating, Input, Output, PushPull};
use stm32f4xx_hal::otg_fs::{UsbBusType, USB};
use stm32f4xx_hal::prelude::*;
use stm32f4xx_hal::stm32::EXTI;
use usb_device::bus::UsbBusAllocator;
use usb_device::prelude::*;

type RcUsbDevice = UsbDevice<'static, UsbBusType>;
type RcUsbClass = HIDClass<'static, UsbBusType>;

const CORE_FREQUENCY_MHZ: u32 = 84;
const REPORT_PERIOD: u32 = 84_000;


#[cfg(rtt)]
use rtt_target::{rprintln, rtt_init_print};

#[app(device = stm32f4xx_hal::stm32, peripherals = true, monotonic = rtic::cyccnt::CYCCNT)]
const APP: () = {
    struct Resources {
        //timer: timer::Timer<stm32::TIM3>,
        usb_device: RcUsbDevice,
        usb_class: RcUsbClass,
        ppm_parser: PpmParser,
        ppm_pin: PA0<Input<Floating>>,
        last_frame: PpmFrame,
        exti: EXTI,
        dwt: rtic::export::DWT,
    }

    #[init(schedule=[report])]
    fn init(mut cx: init::Context) -> init::LateResources {
        static mut EP_MEMORY: [u32; 1024] = [0; 1024];
        static mut USB_BUS: Option<UsbBusAllocator<UsbBusType>> = None;

        #[cfg(rtt_target)]
        rtt_init_print!();

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
        //ppm_parser.set_channel_limits(500, 1500);
        //ppm_parser.set_minimum_channels(12);
        //ppm_parser.set_sync_width(4000);
        //ppm_parser.set_max_ppm_time(core::u32::MAX / CORE_FREQUENCY_MHZ);

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
            dwt: cx.core.DWT,
            last_frame: PpmFrame {
                chan_values: [1400; 20],
                chan_count: 16,
            },
        }
    }

    #[task(binds = EXTI0, resources = [ppm_parser, ppm_pin, dwt], priority = 3)]
    fn ppm_falling(cx: ppm_falling::Context) {
        cx.resources.ppm_pin.clear_interrupt_pending_bit();
        
        #[cfg(rtt)]
        rprintln!("Hi");
        let cycles = cx.resources.dwt.cyccnt.read();
        cx.resources.ppm_parser.handle_pulse_start(cycles / CORE_FREQUENCY_MHZ);
    }

    // Periodic status update to Computer (every millisecond)
    #[task(resources = [usb_class, ppm_parser, last_frame], schedule=[report], priority = 1)]
    fn report(mut cx: report::Context) {
        // schedule itself to keep the loop running
        cx.schedule
            .report(cx.scheduled + REPORT_PERIOD.cycles())
            .unwrap();

        let last_frame = cx.resources.last_frame;

        cx.resources.ppm_parser.lock(|parser: &mut PpmParser| {
            if let Some(frame) = parser.next_frame() {
                *last_frame = frame;
            }
        });

        //TODO commented out
        //cx.resources.usb_class.lock(|class| {
        //    class.write(&get_report(&[1400; 16]));

        //Lock usb_class object and report
        cx.resources.usb_class.lock(|class| {
            class.write(&get_report(
                &last_frame.chan_values[..16].try_into().unwrap(),
            ))
        });

        //TODO Zum Testen
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
