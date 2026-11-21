//! VGA text-mode terminal (80x25, framebuffer 0xB8000, CP437).
//! 支持英文与中文（预览版）双语输出，中文通过 VGA 字模重编程显示。

use core::fmt;
use core::sync::atomic::{AtomicBool, Ordering};

/// 当前界面语言：false = English，true = 中文（预览版）
static CHINESE: AtomicBool = AtomicBool::new(false);

/// 是否启用中文输出
pub fn lang_zh() -> bool {
    CHINESE.load(Ordering::Relaxed)
}

/// 切换界面语言。切换到中文时预装字模并清屏，避免旧内容显示错位。
pub fn set_lang(zh: bool) {
    if zh && !lang_zh() {
        crate::zhfont::init();
    }
    CHINESE.store(zh, Ordering::Relaxed);
    with_writer(|w| w.clear());
}

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
    pub const fn code(self) -> u8 {
        self as u8
    }
}

pub trait Console: fmt::Write {
    fn clear(&mut self);
    fn set_color(&mut self, fg: Color, bg: Color);
    fn put_byte(&mut self, byte: u8);
    fn put_str(&mut self, s: &str);
    fn backspace(&mut self);
    fn update_cursor(&mut self);
    fn set_pos(&mut self, row: usize, col: usize);
    /// Write any raw byte (CP437 glyphs) at the cursor without filtering.
    fn put_raw(&mut self, byte: u8);
    /// Write a high character-code byte (remapped Chinese glyph slot).
    fn put_glyph_byte(&mut self, byte: u8);
}

pub struct TextConsole {
    row: usize,
    col: usize,
    color: u8,
}

impl TextConsole {
    pub const fn new() -> Self {
        Self { row: 0, col: 0, color: 0x0F }
    }

    pub fn reset(&mut self) {
        TextConsole::clear(self);
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
        TextConsole::update_cursor(self);
    }

    pub fn set_color(&mut self, fg: Color, bg: Color) {
        self.color = (fg.code() & 0x0F) | ((bg.code() & 0x0F) << 4);
    }

    pub fn put_byte(&mut self, byte: u8) {
        match byte {
            b'\n' => self.newline(),
            b'\r' => self.col = 0,
            0x08 => TextConsole::backspace(self),
            b'\t' => {
                self.col = (self.col / 8 + 1) * 8;
                if self.col >= WIDTH {
                    self.newline();
                }
            }
            0x20..=0x7E => {
                if self.col >= WIDTH {
                    self.newline();
                }
                self.write_char_at(self.row, self.col, byte);
                self.col += 1;
            }
            _ => {}
        }
        TextConsole::update_cursor(self);
    }

    pub fn put_str(&mut self, s: &str) {
        let bytes = s.as_bytes();
        let mut i = 0usize;
        while i < bytes.len() {
            let b = bytes[i];
            if b < 0x80 {
                TextConsole::put_byte(self, b);
                i += 1;
            } else if b < 0xE0 {
                // 2 字节 UTF-8：系统文案未使用，跳过
                i += 2;
            } else if b < 0xF0 && i + 2 < bytes.len() {
                let cp = ((b as u32 & 0x0F) << 12)
                    | ((bytes[i + 1] as u32 & 0x3F) << 6)
                    | (bytes[i + 2] as u32 & 0x3F);
                if lang_zh() {
                    match crate::zhfont::glyph_bytes(cp) {
                        Some((lo, hi)) => {
                            TextConsole::put_glyph_byte(self, lo);
                            TextConsole::put_glyph_byte(self, hi);
                        }
                        None => TextConsole::put_byte(self, b'?'),
                    }
                }
                i += 3;
            } else {
                i += 1;
            }
        }
    }

    /// 写入一个重映射的高位字符码（汉字半字模）。
    pub fn put_glyph_byte(&mut self, byte: u8) {
        if self.col >= WIDTH {
            self.newline();
        }
        self.write_char_at(self.row, self.col, byte);
        self.col += 1;
        TextConsole::update_cursor(self);
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
        TextConsole::update_cursor(self);
    }

    pub fn put_raw(&mut self, byte: u8) {
        if self.col >= WIDTH {
            self.newline();
        }
        self.write_char_at(self.row, self.col, byte);
        self.col += 1;
    }

    pub fn set_pos(&mut self, row: usize, col: usize) {
        if row < HEIGHT && col < WIDTH {
            self.row = row;
            self.col = col;
        }
    }

    pub fn update_cursor(&self) {
        let pos = self.row * WIDTH + self.col;
        unsafe {
            crate::io::outb(0x3D4, 0x0F);
            crate::io::outb(0x3D5, (pos & 0xFF) as u8);
            crate::io::outb(0x3D4, 0x0E);
            crate::io::outb(0x3D5, ((pos >> 8) & 0xFF) as u8);
        }
    }
}

impl Console for TextConsole {
    fn clear(&mut self) {
        TextConsole::clear(self);
    }
    fn set_color(&mut self, fg: Color, bg: Color) {
        TextConsole::set_color(self, fg, bg);
    }
    fn put_byte(&mut self, byte: u8) {
        TextConsole::put_byte(self, byte);
    }
    fn put_str(&mut self, s: &str) {
        TextConsole::put_str(self, s);
    }
    fn backspace(&mut self) {
        TextConsole::backspace(self);
    }
    fn update_cursor(&mut self) {
        TextConsole::update_cursor(self);
    }
    fn set_pos(&mut self, row: usize, col: usize) {
        TextConsole::set_pos(self, row, col);
    }
    fn put_raw(&mut self, byte: u8) {
        TextConsole::put_raw(self, byte);
    }
    fn put_glyph_byte(&mut self, byte: u8) {
        TextConsole::put_glyph_byte(self, byte);
    }
}

impl fmt::Write for TextConsole {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        TextConsole::put_str(self, s);
        Ok(())
    }
}

static mut CONSOLE: TextConsole = TextConsole::new();

/// Single entry point: all output goes through the active console.
pub fn with_writer<F, R>(f: F) -> R
where
    F: FnOnce(&mut dyn Console) -> R,
{
    unsafe { f(&mut CONSOLE) }
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

/// 双语字符串：根据当前语言返回英文或中文（预览版）。
#[macro_export]
macro_rules! tr {
    ($en:expr, $zh:expr) => {
        if $crate::vga::lang_zh() { $zh } else { $en }
    };
}

/// 双语 println：根据当前语言输出英文或中文。
#[macro_export]
macro_rules! tprintln {
    ($en:literal, $zh:literal $(, $arg:expr)* $(,)?) => {{
        if $crate::vga::lang_zh() {
            $crate::println!($zh $(, $arg)*);
        } else {
            $crate::println!($en $(, $arg)*);
        }
    }};
}

/// 双语 print：根据当前语言输出英文或中文。
#[macro_export]
macro_rules! tprint {
    ($en:literal, $zh:literal $(, $arg:expr)* $(,)?) => {{
        if $crate::vga::lang_zh() {
            $crate::print!($zh $(, $arg)*);
        } else {
            $crate::print!($en $(, $arg)*);
        }
    }};
}
