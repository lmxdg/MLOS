//! VGA 文本模式驱动（80x25，显存 0xB8000）

use crate::io::outb;
use core::fmt;

pub const WIDTH: usize = 80;
pub const HEIGHT: usize = 25;
const VGA_ADDR: usize = 0xB8000;

#[derive(Clone, Copy)]
#[repr(u8)]
pub enum Color {
    Black = 0,
    Blue = 1,
    Green = 2,
    Cyan = 3,
    Red = 4,
    Magenta = 5,
    Brown = 6,
    LightGray = 7,
    DarkGray = 8,
    LightBlue = 9,
    LightGreen = 10,
    LightCyan = 11,
    LightRed = 12,
    Pink = 13,
    Yellow = 14,
    White = 15,
}

impl Color {
    const fn code(self) -> u8 {
        self as u8
    }
}

pub struct Writer {
    row: usize,
    col: usize,
    color: u8,
}

impl Writer {
    pub const fn new() -> Self {
        Self { row: 0, col: 0, color: 0x0F }
    }

    pub fn set_color(&mut self, fg: Color, bg: Color) {
        self.color = (fg.code() & 0x0F) | ((bg.code() & 0x0F) << 4);
    }

    pub fn clear(&mut self) {
        let color = self.color;
        unsafe {
            let buf = VGA_ADDR as *mut u16;
            for i in 0..(WIDTH * HEIGHT) {
                core::ptr::write_volatile(buf.add(i), ((color as u16) << 8) | b' ' as u16);
            }
            // 用 volatile 写，避免编译器把 row/col 合并成 SSE 指令
            core::ptr::write_volatile(core::ptr::addr_of_mut!(self.row), 0);
            core::ptr::write_volatile(core::ptr::addr_of_mut!(self.col), 0);
        }
        self.update_cursor();
    }

    pub fn put_byte(&mut self, byte: u8) {
        match byte {
            b'\n' => self.newline(),
            b'\r' => self.col = 0,
            0x08 => self.backspace(),
            b'\t' => {
                self.col = (self.col / 8 + 1) * 8;
                if self.col >= WIDTH {
                    self.newline();
                }
            }
            0x20..=0x7E | 0x80..=0xFF => {
                if self.col >= WIDTH {
                    self.newline();
                }
                self.write_char_at(self.row, self.col, byte);
                self.col += 1;
            }
            _ => {}
        }
        self.update_cursor();
    }

    pub fn put_str(&mut self, s: &str) {
        for b in s.bytes() {
            self.put_byte(b);
        }
    }

    fn write_char_at(&self, row: usize, col: usize, byte: u8) {
        let idx = row * WIDTH + col;
        unsafe {
            let buf = VGA_ADDR as *mut u16;
            core::ptr::write_volatile(buf.add(idx), ((self.color as u16) << 8) | byte as u16);
        }
    }

    fn newline(&mut self) {
        self.col = 0;
        self.row += 1;
        if self.row >= HEIGHT {
            self.scroll();
        }
    }

    fn scroll(&mut self) {
        unsafe {
            let buf = VGA_ADDR as *mut u16;
            for i in 0..(WIDTH * (HEIGHT - 1)) {
                let v = core::ptr::read_volatile(buf.add(i + WIDTH));
                core::ptr::write_volatile(buf.add(i), v);
            }
            for i in 0..WIDTH {
                core::ptr::write_volatile(
                    buf.add(WIDTH * (HEIGHT - 1) + i),
                    ((self.color as u16) << 8) | b' ' as u16,
                );
            }
        }
        self.row = HEIGHT - 1;
    }

    pub fn backspace(&mut self) {
        if self.col > 0 {
            self.col -= 1;
            self.write_char_at(self.row, self.col, b' ');
        }
        self.update_cursor();
    }

    pub fn update_cursor(&self) {
        let pos = self.row * WIDTH + self.col;
        unsafe {
            outb(0x3D4, 0x0F);
            outb(0x3D5, (pos & 0xFF) as u8);
            outb(0x3D4, 0x0E);
            outb(0x3D5, ((pos >> 8) & 0xFF) as u8);
        }
    }
}

impl fmt::Write for Writer {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.put_str(s);
        Ok(())
    }
}

static mut WRITER: Writer = Writer::new();

pub fn with_writer<F, R>(f: F) -> R
where
    F: FnOnce(&mut Writer) -> R,
{
    unsafe { f(&mut *core::ptr::addr_of_mut!(WRITER)) }
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {{
        $crate::vga::with_writer(|w| {
            use core::fmt::Write;
            let _ = w.write_fmt(format_args!($($arg)*));
        });
    }};
}

#[macro_export]
macro_rules! println {
    ($($arg:tt)*) => {{
        $crate::vga::with_writer(|w| {
            use core::fmt::Write;
            let _ = w.write_fmt(format_args!($($arg)*));
            let _ = w.write_char('\n');
        });
    }};
    () => {
        $crate::println!("")
    };
}
