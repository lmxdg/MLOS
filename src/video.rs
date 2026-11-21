//! 显示模式检测与报告。
//!
//! 引导程序通过 Multiboot2 framebuffer tag（type 8）报告 VBE 线性帧缓冲；
//! `fb` 负责解析与绘图。没有帧缓冲时回退到 VGA 文本模式，并通过 CRTC
//! 寄存器探测实际的文本行列数。

use crate::fb;
use crate::io::{inb, outb};

pub struct ConsoleMode {
    pub cols: usize,
    pub rows: usize,
    pub cell_h: usize,
}

#[derive(Clone, Copy)]
pub struct AdapterInfo {
    pub present: bool,
    pub width: u64,
    pub height: u64,
    pub bpp: u8,
    pub kind: &'static str,
}

static mut ADAPTER: AdapterInfo = AdapterInfo {
    present: false,
    width: 0,
    height: 0,
    bpp: 0,
    kind: "none",
};
static mut CONSOLE: ConsoleMode = ConsoleMode { cols: 80, rows: 25, cell_h: 16 };

pub fn init(mbi: u64) {
    fb::init(mbi);
    let adapter;
    let console;

    if fb::available() {
        adapter = AdapterInfo {
            present: true,
            width: fb::width() as u64,
            height: fb::height() as u64,
            bpp: fb::bpp() as u8,
            kind: "VBE linear framebuffer",
        };
        console = ConsoleMode {
            cols: (fb::width() / 8).max(40),
            rows: (fb::height() / 16).max(15),
            cell_h: 16,
        };
    } else {
        adapter = AdapterInfo { present: false, width: 0, height: 0, bpp: 0, kind: "none" };
        // 探测 VGA 文本控制台
        let cols = crtc_read(0x01) as usize + 1;
        let line_h = ((crtc_read(0x09) >> 4) as usize & 0x0F) + 1;
        let mut vde = crtc_read(0x12) as usize;
        let ovf = crtc_read(0x07);
        vde |= ((ovf as usize) & 0x02) << 7;
        vde |= ((ovf as usize) & 0x40) << 3;
        let rows = if line_h > 0 { (vde + 1) / line_h } else { 25 };
        if (40..=120).contains(&cols) && (20..=60).contains(&rows) {
            console = ConsoleMode { cols, rows, cell_h: line_h };
        } else {
            console = ConsoleMode { cols: 80, rows: 25, cell_h: line_h };
        }
    }

    unsafe {
        ADAPTER = adapter;
        CONSOLE = console;
    }
}

#[inline]
fn crtc_read(index: u8) -> u8 {
    unsafe {
        outb(0x3D4, index);
        inb(0x3D5)
    }
}

pub fn console() -> (usize, usize) {
    unsafe {
        let c = CONSOLE.cols;
        let r = CONSOLE.rows;
        (c, r)
    }
}

pub fn adapter() -> AdapterInfo {
    unsafe {
        let a = ADAPTER;
        a
    }
}

pub fn adapter_present() -> bool {
    unsafe { ADAPTER.present }
}

pub fn describe() -> &'static str {
    unsafe {
        if ADAPTER.present {
            if ADAPTER.width >= 1024 {
                "VBE framebuffer (1024x768+), bitmap font"
            } else {
                "VBE framebuffer, bitmap font"
            }
        } else if CONSOLE.cols == 80 && CONSOLE.rows == 25 {
            "VGA text mode 80x25 (fallback)"
        } else {
            "VGA text mode (fallback)"
        }
    }
}

pub fn report(buf: &mut alloc::string::String) {
    use core::fmt::Write;
    unsafe {
        let w = ADAPTER.width;
        let h = ADAPTER.height;
        let bpp = ADAPTER.bpp;
        let kind = ADAPTER.kind;
        let cols = CONSOLE.cols;
        let rows = CONSOLE.rows;
        let cell = CONSOLE.cell_h;
        let _ = write!(buf, "  Adapter     : ");
        if ADAPTER.present {
            let _ = write!(buf, "{}x{} @ {} bpp ({})\n", w, h, bpp, kind);
        } else {
            let _ = write!(buf, "not reported by firmware (VGA text mode)\n");
        }
        let _ = write!(
            buf,
            "  Console     : {}x{} characters (cell height {}px, auto-detected)\n",
            cols, rows, cell
        );
        let _ = write!(
            buf,
            "  Renderer    : framebuffer bitmap font (ASCII 8x16 + CJK 16x16)\n"
        );
        let _ = write!(
            buf,
            "                VGA text mode + dynamic font is used only as fallback.\n"
        );
    }
}