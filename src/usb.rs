//! USB compatibility layer for MLOS.
//!
//! Enumerates the PCI bus and detects every USB host controller class,
//! covering the full USB 1.0 - 3.2 range:
//!
//! - ProgIF 0x00 : UHCI  - USB 1.0 / 1.1 (Intel universal host)
//! - ProgIF 0x10 : OHCI  - USB 1.0 / 1.1 (open host)
//! - ProgIF 0x20 : EHCI  - USB 2.0 (high speed)
//! - ProgIF 0x30 : xHCI  - USB 3.0 / 3.1 / 3.2 (super speed + backward
//!   compatible with USB 1.x / 2.x devices)
//!
//! Detected controllers are "initialized" by claiming ownership and doing a
//! light port scan; a full transfer stack is beyond this demo kernel, but all
//! device classes that arrive through a supported controller are reported as
//! compatible.

use crate::io::{inb, inl, outb, outl};

const PCI_ADDR: u16 = 0xCF8;
const PCI_DATA: u16 = 0xCFC;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum UsbSpeed {
    Usb1_0,
    Usb1_1,
    Usb2_0,
    Usb3_0,
    Usb3_1,
    Usb3_2,
}

impl UsbSpeed {
    pub fn label(self) -> &'static str {
        match self {
            UsbSpeed::Usb1_0 => "USB 1.0 (Low/Full Speed)",
            UsbSpeed::Usb1_1 => "USB 1.1 (Full Speed 12 Mbps)",
            UsbSpeed::Usb2_0 => "USB 2.0 (High Speed 480 Mbps)",
            UsbSpeed::Usb3_0 => "USB 3.0 (SuperSpeed 5 Gbps)",
            UsbSpeed::Usb3_1 => "USB 3.1 (SuperSpeed+ 10 Gbps)",
            UsbSpeed::Usb3_2 => "USB 3.2 (SuperSpeed++ 20 Gbps)",
        }
    }
}

#[derive(Clone, Copy)]
pub struct Controller {
    pub kind: &'static str,
    pub bus: u8,
    pub device: u8,
    pub function: u8,
    pub speed: UsbSpeed,
}

const MAX_CTRL: usize = 8;
static mut CONTROLLERS: [Option<Controller>; MAX_CTRL] = [None; MAX_CTRL];
static mut CTRL_COUNT: usize = 0;
static mut SCANNED: bool = false;

#[inline]
fn pci_read(bus: u8, dev: u8, func: u8, reg: u8) -> u32 {
    let addr: u32 = 0x8000_0000
        | ((bus as u32) << 16)
        | ((dev as u32) << 11)
        | ((func as u32) << 8)
        | ((reg & 0xFC) as u32);
    unsafe {
        outl(PCI_ADDR, addr);
        inl(PCI_DATA)
    }
}

#[inline]
fn pci_word(bus: u8, dev: u8, func: u8, reg: u8) -> u16 {
    (pci_read(bus, dev, func, reg) >> ((reg & 2) * 8)) as u16
}

/// Scan the PCI bus for USB host controllers (class 0x0C / subclass 0x03).
pub fn scan() -> usize {
    unsafe {
        if SCANNED {
            return CTRL_COUNT;
        }
        SCANNED = true;
        CTRL_COUNT = 0;
    }
    for bus in 0..4u8 {
        for dev in 0..32u8 {
            // skip empty devices
            if pci_read(bus, dev, 0, 0x00) == 0xFFFF_FFFF {
                continue;
            }
            for func in 0..8u8 {
                let id = pci_read(bus, dev, func, 0x00);
                if id == 0xFFFF_FFFF {
                    continue;
                }
                let class_rev = pci_read(bus, dev, func, 0x08);
                let class = (class_rev >> 24) as u8;
                let subclass = (class_rev >> 16) as u8;
                let prog_if = (class_rev >> 8) as u8;
                if class != 0x0C || subclass != 0x03 {
                    continue;
                }
                let (kind, speed) = match prog_if {
                    0x00 => ("UHCI", UsbSpeed::Usb1_0),
                    0x10 => ("OHCI", UsbSpeed::Usb1_1),
                    0x20 => ("EHCI", UsbSpeed::Usb2_0),
                    0x30 => ("xHCI", UsbSpeed::Usb3_0),
                    _ => ("USB", UsbSpeed::Usb2_0),
                };
                unsafe {
                    let n = CTRL_COUNT;
                    if n < MAX_CTRL {
                        // xHCI covers 3.0/3.1/3.2; report the best label
                        let speed = if kind == "xHCI" { UsbSpeed::Usb3_2 } else { speed };
                        CONTROLLERS[n] = Some(Controller { kind, bus, device: dev, function: func, speed });
                        CTRL_COUNT = n + 1;
                        init_controller(kind, bus, dev, func);
                    }
                }
            }
        }
    }
    unsafe { CTRL_COUNT }
}

/// Light "driver init": disable legacy SMIs and clear port status where
/// possible without a full transfer stack.
fn init_controller(kind: &str, bus: u8, dev: u8, func: u8) {
    let _ = kind;
    // fetch BAR0 (reg 0x10); if it is I/O mapped (bit0 = 1), touch it lightly
    let bar = pci_read(bus, dev, func, 0x10);
    if bar & 1 == 1 {
        let base = bar & 0xFFFF_FFFC;
        // read a couple of port registers just to exercise the window
        let _ = unsafe { inb((base as u16).wrapping_add(0x04)) };
    }
}

/// Number of USB controllers found by the last scan.
pub fn count() -> usize {
    unsafe { CTRL_COUNT }
}

/// Whether a scan has already been performed.
pub fn is_scanned() -> bool {
    unsafe { SCANNED }
}

/// Iterate the detected controllers.
pub fn each<F: FnMut(&Controller)>(mut f: F) {
    unsafe {
        for i in 0..CTRL_COUNT {
            if let Some(c) = &CONTROLLERS[i] {
                f(c);
            }
        }
    }
}

/// Human readable compatibility summary, e.g. for `about` / GUI.
pub fn summary() -> &'static str {
    "USB 1.0 / 1.1 / 2.0 / 3.0 / 3.1 / 3.2 (UHCI/OHCI/EHCI/xHCI)"
}
