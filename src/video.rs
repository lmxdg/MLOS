//! Adaptive display resolution detection for MLOS.
//!
//! Two levels of detection:
//! 1. Firmware / bootloader mode - read the Multiboot2 framebuffer tag
//!    (type 8). When GRUB or the firmware provides a framebuffer, its
//!    width/height/bpp are stored and reported as the adapter capability.
//! 2. Active console mode - probe the VGA CRTC registers to derive the real
//!    columns/rows/cell-height of the text console the kernel is drawing to.
//!
//! The GUI and shell use the detected console metrics; the adapter info is
//! surfaced via the `resolution` shell command and the boot banner.

use crate::io::{inb, outb};

pub struct ConsoleMode {
    pub cols: usize,
    pub rows: usize,
    pub cell_h: usize,
}

pub struct AdapterInfo {
    pub present: bool,
    pub width: u64,
    pub height: u64,
    pub bpp: u8,
    pub kind: &'static str,
}

static mut ADAPTER: AdapterInfo = AdapterInfo { present: false, width: 0, height: 0, bpp: 0, kind: "none" };
static mut CONSOLE: ConsoleMode = ConsoleMode { cols: 80, rows: 25, cell_h: 16 };

/// Parse the Multiboot2 info for a framebuffer tag (type 8) and probe the
/// VGA CRTC for the active text console geometry.
pub fn init(mbi: u64) {
    unsafe {
        // --- framebuffer from bootloader (type 8) ---
        if mbi != 0 {
            let total = *(mbi as *const u32) as usize;
            let mut off = 8usize;
            while off + 8 <= total {
                let tag_type = *((mbi + off as u64) as *const u32);
                let tag_size = *((mbi + off as u64 + 4) as *const u32) as usize;
                if tag_type == 0 || tag_size < 8 {
                    break;
                }
                if tag_type == 8 && tag_size >= 24 {
                    let width = *((mbi + off as u64 + 8) as *const u32) as u64;
                    let height = *((mbi + off as u64 + 12) as *const u32) as u64;
                    let bpp = *((mbi + off as u64 + 16) as *const u8);
                    let kind = *((mbi + off as u64 + 20) as *const u8);
                    if width > 0 && height > 0 {
                        let k = match kind {
                            1 => "RGB framebuffer",
                            2 => "VGA text framebuffer",
                            _ => "firmware framebuffer",
                        };
                        ADAPTER = AdapterInfo { present: true, width, height, bpp, kind: k };
                    }
                }
                off = (off + tag_size + 7) & !7;
            }
        }

        // --- active VGA text console via CRTC ---
        let cols = crtc_read(0x01) as usize + 1; // Horizontal Display End
        let line_h = ((crtc_read(0x09) >> 4) as usize & 0x0F) + 1; // Maximum Scan Line
        // Vertical Display End: reg 0x12 (7 bits) + overflow bits
        let mut vde = crtc_read(0x12) as usize;
        let ovf = crtc_read(0x07);
        vde |= ((ovf as usize) & 0x02) << 7; // bit 8  -> overflow bit 1
        vde |= ((ovf as usize) & 0x40) << 3; // bit 9  -> overflow bit 6
        let rows = if line_h > 0 { (vde + 1) / line_h } else { 25 };
        // sanity check - fall back to the standard 80x25
        if (40..=120).contains(&cols) && (20..=60).contains(&rows) {
            CONSOLE = ConsoleMode { cols, rows, cell_h: line_h };
        } else {
            CONSOLE = ConsoleMode { cols: 80, rows: 25, cell_h: line_h };
        }
    }
}

#[inline]
fn crtc_read(index: u8) -> u8 {
    unsafe {
        outb(0x3D4, index);
        inb(0x3D5)
    }
}

/// Detected active console geometry.
pub fn console() -> (usize, usize) {
    unsafe { (CONSOLE.cols, CONSOLE.rows) }
}

/// Firmware adapter info (may be absent in pure text mode).
pub fn adapter_str() -> &'static str {
    unsafe {
        if ADAPTER.present {
            "unknown"
        } else {
            "none (text mode)"
        }
    }
}

/// Short description shown in the banner / about.
pub fn describe() -> &'static str {
    unsafe {
        let (c, r) = (CONSOLE.cols, CONSOLE.rows);
        if ADAPTER.present {
            "adaptive (firmware framebuffer + VGA text console)"
        } else if c == 80 && r == 25 {
            "VGA text mode 80x25 (auto-detected)"
        } else {
            "VGA text mode (auto-detected)"
        }
    }
}

/// Detailed multi-line report for the `resolution` command.
pub fn report(buf: &mut alloc::string::String) {
    use core::fmt::Write;
    unsafe {
        let _ = write!(buf, "  Adapter     : ");
        if ADAPTER.present {
            let _ = write!(
                buf,
                "{}x{} @ {} bpp ({})\n",
                ADAPTER.width, ADAPTER.height, ADAPTER.bpp, ADAPTER.kind
            );
        } else {
            let _ = write!(buf, "not reported by firmware (text mode)\n");
        }
        let _ = write!(
            buf,
            "  Console     : {}x{} characters (cell height {}px, auto-detected)\n",
            CONSOLE.cols, CONSOLE.rows, CONSOLE.cell_h
        );
        let _ = write!(
            buf,
            "  Policy      : adaptive - display metrics detected at boot,\n"
        );
        let _ = write!(
            buf,
            "                GUI layout follows the detected console size.\n"
        );
    }
}
