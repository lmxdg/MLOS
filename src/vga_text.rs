//! 渲染后端一：VGA 文本模式（80x25，显存 0xB8000）
//! 经典 CP437 字符输出，用于英文界面。

use crate::io::outb;
use crate::vga::{Color, Console};
use core::fmt;

pub const WIDTH: usize = 80;
pub const HEIGHT: usize = 25;
const VGA_ADDR: usize = 0xB8000;

pub struct VgaTextConsole {
    row: usize,
    col: usize,
    color: u8,
}

impl VgaTextConsole {
    pub const fn new() -> Self {
        Self { row: 0, col: 0, color: 0x0F }
    }

    /// 切回文本模式后复位：清屏并归零光标。
    pub fn reset(&mut self) {
        self.clear();
    }

    pub fn clear(&mut self) {
        let color = self.color;
        unsafe {
            let buf = VGA_ADDR as *mut u16;
            for i in 0..(WIDTH * HEIGHT) {
                core::ptr::write_volatile(buf.add(i), ((color as u16) << 8) | b' ' as u16);
            }
            core::ptr::write_volatile(core::ptr::addr_of_mut!(self.row), 0);
            core::ptr::write_volatile(core::ptr::addr_of_mut!(self.col), 0);
        }
        self.update_cursor();
    }

    pub fn set_color(&mut self, fg: Color, bg: Color) {
        self.color = (fg.code() & 0x0F) | ((bg.code() & 0x0F) << 4);
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

impl Console for VgaTextConsole {
    fn clear(&mut self) {
        VgaTextConsole::clear(self);
    }
    fn set_color(&mut self, fg: Color, bg: Color) {
        VgaTextConsole::set_color(self, fg, bg);
    }
    fn put_byte(&mut self, byte: u8) {
        VgaTextConsole::put_byte(self, byte);
    }
    fn put_str(&mut self, s: &str) {
        VgaTextConsole::put_str(self, s);
    }
    fn backspace(&mut self) {
        VgaTextConsole::backspace(self);
    }
    fn update_cursor(&mut self) {
        VgaTextConsole::update_cursor(self);
    }
}

impl fmt::Write for VgaTextConsole {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.put_str(s);
        Ok(())
    }
}
