//! 控制台抽象层。
//!
//! 首选 framebuffer 控制台（直接绘制 8x16 ASCII + 16x16 中文点阵）；
//! 无线性 framebuffer 时回退到 VGA 文本模式（80x25，CP437，中文通过
//! 0xA0000 字符发生器 RAM 的动态字模显示）。
//!
//! 上层 Shell / 安装向导 / 游戏只依赖 `Console` trait，不关心底层模式。

use core::fmt;
use core::sync::atomic::{AtomicBool, Ordering};

/// 当前界面语言：false = English，true = 中文
static CHINESE: AtomicBool = AtomicBool::new(false);
static FB_READY: AtomicBool = AtomicBool::new(false);

pub fn lang_zh() -> bool {
    CHINESE.load(Ordering::Relaxed)
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
    /// 写原始字符码（文本模式下为 CP437 / 字模槽）。
    fn put_raw(&mut self, byte: u8);
    /// 写字模槽高位字节（文本模式中文回退路径使用）。
    fn put_glyph_byte(&mut self, byte: u8);
}

// ===========================================================================
// VGA 文本模式回退控制台
// ===========================================================================

pub struct TextConsole {
    row: usize,
    col: usize,
    color: u8,
}

impl TextConsole {
    pub const fn new() -> Self {
        Self { row: 0, col: 0, color: 0x0F }
    }

    pub fn clear(&mut self) {
        let color = self.color;
        unsafe {
            let buf = VGA_ADDR as *mut u16;
            for i in 0..(WIDTH * HEIGHT) {
                core::ptr::write_volatile(buf.add(i), ((color as u16) << 8) | b' ' as u16);
            }
        }
        self.row = 0;
        self.col = 0;
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
            0x20..=0x7E => {
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
        let bytes = s.as_bytes();
        let mut i = 0usize;
        while i < bytes.len() {
            let b = bytes[i];
            if b < 0x80 {
                self.put_byte(b);
                i += 1;
            } else if b < 0xE0 {
                i += 2;
            } else if b < 0xF0 && i + 2 < bytes.len() {
                let cp = ((b as u32 & 0x0F) << 12)
                    | ((bytes[i + 1] as u32 & 0x3F) << 6)
                    | (bytes[i + 2] as u32 & 0x3F);
                if lang_zh() {
                    match crate::zhfont::glyph_bytes(cp) {
                        Some((lo, hi)) => {
                            self.put_glyph_byte(lo);
                            self.put_glyph_byte(hi);
                        }
                        None => self.put_byte(b'?'),
                    }
                }
                i += 3;
            } else {
                i += 1;
            }
        }
    }

    pub fn put_glyph_byte(&mut self, byte: u8) {
        if self.col >= WIDTH {
            self.newline();
        }
        self.write_char_at(self.row, self.col, byte);
        self.col += 1;
        self.update_cursor();
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

    pub fn put_raw(&mut self, byte: u8) {
        if self.col >= WIDTH {
            self.newline();
        }
        self.write_char_at(self.row, self.col, byte);
        self.col += 1;
        self.update_cursor();
    }

    pub fn set_pos(&mut self, row: usize, col: usize) {
        if row < HEIGHT && col < WIDTH {
            self.row = row;
            self.col = col;
        }
        self.update_cursor();
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

    pub fn hide_cursor(&self) {
        unsafe {
            crate::io::outb(0x3D4, 0x0A);
            crate::io::outb(0x3D5, 0x20);
        }
    }
}

impl Console for TextConsole {
    fn clear(&mut self) { TextConsole::clear(self); }
    fn set_color(&mut self, fg: Color, bg: Color) { TextConsole::set_color(self, fg, bg); }
    fn put_byte(&mut self, byte: u8) { TextConsole::put_byte(self, byte); }
    fn put_str(&mut self, s: &str) { TextConsole::put_str(self, s); }
    fn backspace(&mut self) { TextConsole::backspace(self); }
    fn update_cursor(&mut self) { TextConsole::update_cursor(self); }
    fn set_pos(&mut self, row: usize, col: usize) { TextConsole::set_pos(self, row, col); }
    fn put_raw(&mut self, byte: u8) { TextConsole::put_raw(self, byte); }
    fn put_glyph_byte(&mut self, byte: u8) { TextConsole::put_glyph_byte(self, byte); }
}

impl fmt::Write for TextConsole {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        TextConsole::put_str(self, s);
        Ok(())
    }
}

// ===========================================================================
// Framebuffer 控制台
// ===========================================================================

const MAX_COLS: usize = 160;
const MAX_ROWS: usize = 60;
const CELL_W: usize = 8;
const CELL_H: usize = 16;

#[derive(Clone, Copy)]
struct Cell {
    cp: u32,
    fg: u8,
    bg: u8,
    /// 双宽字符的右半格
    cont: bool,
}

const BLANK: Cell = Cell { cp: 0, fg: 7, bg: 0, cont: false };

pub struct FbConsole {
    cols: usize,
    rows: usize,
    row: usize,
    col: usize,
    color: u8,
    cells: [[Cell; MAX_COLS]; MAX_ROWS],
    cursor_visible: bool,
    cursor_row: usize,
    cursor_col: usize,
}

impl FbConsole {
    const fn new() -> Self {
        Self {
            cols: 0,
            rows: 0,
            row: 0,
            col: 0,
            color: 0x0F,
            cells: [[BLANK; MAX_COLS]; MAX_ROWS],
            cursor_visible: false,
            cursor_row: 0,
            cursor_col: 0,
        }
    }

    pub fn init(&mut self) {
        self.cols = (crate::fb::width() / CELL_W).clamp(40, MAX_COLS);
        self.rows = (crate::fb::height() / CELL_H).clamp(15, MAX_ROWS);
        self.color = 0x0F;
        self.row = 0;
        self.col = 0;
        self.cursor_visible = false;
        self.clear();
    }

    fn fg_rgb(&self) -> u32 { crate::fb::color(self.color & 0x0F) }
    fn bg_rgb(&self) -> u32 { crate::fb::color(self.color >> 4) }

    fn draw_cell(&self, row: usize, col: usize) {
        if row >= self.rows || col >= self.cols { return; }
        let cell = self.cells[row][col];
        let x = (col * CELL_W) as i32;
        let y = (row * CELL_H) as i32;
        let fg = crate::fb::color(cell.fg);
        let bg = crate::fb::color(cell.bg);
        if cell.cont {
            crate::fb::fill_rect(x, y, CELL_W as i32, CELL_H as i32, bg);
            return;
        }
        if cell.cp == 0 {
            crate::fb::fill_rect(x, y, CELL_W as i32, CELL_H as i32, bg);
        } else if cell.cp < 0x80 {
            if let Some(g) = crate::font::ascii(cell.cp as u8) {
                crate::fb::draw_ascii(x, y, g, fg, bg);
            }
        } else {
            if let Some(g) = crate::font::cjk(cell.cp) {
                crate::fb::draw_cjk(x, y, g, fg, bg);
                if col + 1 >= self.cols {
                    crate::fb::fill_rect(x + 8, y, 8, CELL_H as i32, bg);
                }
            } else {
                if let Some(g) = crate::font::ascii(b'?') {
                    crate::fb::draw_ascii(x, y, g, fg, bg);
                }
                crate::fb::fill_rect(x + 8, y, 8, CELL_H as i32, bg);
            }
        }
    }

    fn store_simple(&mut self, row: usize, col: usize, cp: u32) {
        if row >= self.rows || col >= self.cols { return; }
        self.cells[row][col] = Cell {
            cp,
            fg: self.color & 0x0F,
            bg: self.color >> 4,
            cont: false,
        };
        self.draw_cell(row, col);
    }

    fn store_wide(&mut self, mut row: usize, mut col: usize, cp: u32) {
        if row >= self.rows { return; }
        if col + 1 >= self.cols {
            // 行尾放不下：填一个空格后换行，再在新行首绘制
            self.store_simple(row, col, b' ' as u32);
            self.col = 0;
            self.row += 1;
            if self.row >= self.rows { self.scroll(); }
            row = self.row;
            col = 0;
        }
        let fg = self.color & 0x0F;
        let bg = self.color >> 4;
        self.cells[row][col] = Cell { cp, fg, bg, cont: false };
        self.cells[row][col + 1] = Cell { cp: 0, fg, bg, cont: true };
        self.draw_cell(row, col);
        // 右半格由 draw_cell 一起绘制（draw_cjk 覆盖 16px）
    }

    fn erase_cursor(&mut self) {
        if self.cursor_visible {
            self.cursor_visible = false;
            self.draw_cell(self.cursor_row, self.cursor_col);
        }
    }

    fn draw_cursor_marker(&self) {
        if !self.cursor_visible { return; }
        let x = (self.cursor_col * CELL_W) as i32;
        let y = (self.cursor_row * CELL_H) as i32 + CELL_H as i32 - 2;
        let wide = self.cells[self.cursor_row][self.cursor_col].cp >= 0x80;
        let w = if wide { CELL_W * 2 } else { CELL_W } as i32;
        crate::fb::fill_rect(x, y, w, 2, self.fg_rgb());
    }

    fn show_cursor_now(&mut self) {
        self.cursor_row = self.row.min(self.rows - 1);
        self.cursor_col = self.col.min(self.cols - 1);
        self.cursor_visible = true;
        self.draw_cursor_marker();
    }

    fn newline_internal(&mut self) {
        self.col = 0;
        self.row += 1;
        if self.row >= self.rows {
            self.scroll();
        }
    }

    fn scroll(&mut self) {
        if self.rows <= 1 { return; }
        for r in 0..self.rows - 1 {
            self.cells[r] = self.cells[r + 1];
        }
        self.cells[self.rows - 1] = [Cell { cp: 0, fg: self.color & 0x0F, bg: self.color >> 4, cont: false }; MAX_COLS];
        crate::fb::scroll_up(CELL_H, self.bg_rgb());
        self.row = self.rows - 1;
    }

    fn write_cp(&mut self, cp: u32) {
        if cp < 0x80 {
            self.store_simple(self.row, self.col, cp);
            self.col += 1;
            if self.col >= self.cols { self.newline_internal(); }
        } else if cp < 0x8000_0000 {
            let wide = crate::font::char_width(cp) == crate::font::CJK_W;
            if wide {
                self.store_wide(self.row, self.col, cp);
                self.col += 2;
                if self.col >= self.cols { self.newline_internal(); }
            } else {
                self.store_simple(self.row, self.col, b'?' as u32);
                self.col += 1;
                if self.col >= self.cols { self.newline_internal(); }
            }
        }
    }

    fn handle_byte(&mut self, byte: u8) {
        match byte {
            b'\n' => self.newline_internal(),
            b'\r' => self.col = 0,
            0x08 => self.backspace_no_cursor(),
            b'\t' => {
                let next = (self.col / 8 + 1) * 8;
                while self.col < next && self.col < self.cols {
                    self.store_simple(self.row, self.col, b' ' as u32);
                    self.col += 1;
                    if self.col >= self.cols { self.newline_internal(); break; }
                }
            }
            0x20..=0x7E => self.write_cp(byte as u32),
            _ => {}
        }
    }

    fn backspace_no_cursor(&mut self) {
        if self.col > 0 {
            self.col -= 1;
            if self.cells[self.row][self.col].cont && self.col > 0 {
                self.col -= 1;
            }
            let cp = self.cells[self.row][self.col].cp;
            let wide = cp >= 0x80;
            self.cells[self.row][self.col] = BLANK;
            self.draw_cell(self.row, self.col);
            if wide && self.col + 1 < self.cols {
                self.cells[self.row][self.col + 1] = BLANK;
                self.draw_cell(self.row, self.col + 1);
            }
        }
    }

    pub fn clear(&mut self) {
        let blank = Cell { cp: 0, fg: self.color & 0x0F, bg: self.color >> 4, cont: false };
        for r in 0..MAX_ROWS {
            for c in 0..MAX_COLS {
                self.cells[r][c] = blank;
            }
        }
        self.row = 0;
        self.col = 0;
        self.cursor_visible = false;
        crate::fb::clear(self.bg_rgb());
        self.show_cursor_now();
    }

    pub fn set_color(&mut self, fg: Color, bg: Color) {
        self.color = (fg.code() & 0x0F) | ((bg.code() & 0x0F) << 4);
    }

    pub fn put_byte(&mut self, byte: u8) {
        self.erase_cursor();
        self.handle_byte(byte);
        self.show_cursor_now();
    }

    pub fn put_str(&mut self, s: &str) {
        self.erase_cursor();
        let bytes = s.as_bytes();
        let mut i = 0usize;
        while i < bytes.len() {
            let b = bytes[i];
            if b < 0x80 {
                self.handle_byte(b);
                i += 1;
            } else {
                let (cp, n) = decode_utf8(&bytes[i..]);
                if let Some(cp) = cp {
                    self.write_cp(cp);
                    i += n;
                } else {
                    self.handle_byte(b'?');
                    i += 1;
                }
            }
        }
        self.show_cursor_now();
    }

    pub fn put_glyph_byte(&mut self, byte: u8) {
        self.put_byte(if byte < 0x80 { byte } else { b'?' });
    }

    pub fn backspace(&mut self) {
        self.erase_cursor();
        self.backspace_no_cursor();
        self.show_cursor_now();
    }

    pub fn put_raw(&mut self, byte: u8) {
        self.put_byte(if byte >= 0x20 { byte } else { b'?' });
    }

    pub fn set_pos(&mut self, row: usize, col: usize) {
        self.erase_cursor();
        if row < self.rows && col < self.cols {
            self.row = row;
            self.col = col;
        }
        self.show_cursor_now();
    }

    pub fn update_cursor(&mut self) {
        self.erase_cursor();
        self.show_cursor_now();
    }

    pub fn cursor(&self) -> (usize, usize) { (self.row, self.col) }
}

impl Console for FbConsole {
    fn clear(&mut self) { FbConsole::clear(self); }
    fn set_color(&mut self, fg: Color, bg: Color) { FbConsole::set_color(self, fg, bg); }
    fn put_byte(&mut self, byte: u8) { FbConsole::put_byte(self, byte); }
    fn put_str(&mut self, s: &str) { FbConsole::put_str(self, s); }
    fn backspace(&mut self) { FbConsole::backspace(self); }
    fn update_cursor(&mut self) { FbConsole::update_cursor(self); }
    fn set_pos(&mut self, row: usize, col: usize) { FbConsole::set_pos(self, row, col); }
    fn put_raw(&mut self, byte: u8) { FbConsole::put_raw(self, byte); }
    fn put_glyph_byte(&mut self, byte: u8) { FbConsole::put_glyph_byte(self, byte); }
}

impl fmt::Write for FbConsole {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        FbConsole::put_str(self, s);
        Ok(())
    }
}

/// 解析一个 UTF-8 字符，返回 (码点, 字节数)。
fn decode_utf8(bytes: &[u8]) -> (Option<u32>, usize) {
    if bytes.is_empty() { return (None, 0); }
    let b = bytes[0];
    if b < 0x80 { return (Some(b as u32), 1); }
    if b < 0xC2 { return (None, 1); }
    if b < 0xE0 {
        if bytes.len() < 2 || bytes[1] & 0xC0 != 0x80 { return (None, 1); }
        return (Some(((b as u32 & 0x1F) << 6) | (bytes[1] as u32 & 0x3F)), 2);
    }
    if b < 0xF0 {
        if bytes.len() < 3 || bytes[1] & 0xC0 != 0x80 || bytes[2] & 0xC0 != 0x80 {
            return (None, 1);
        }
        return (Some(((b as u32 & 0x0F) << 12)
            | ((bytes[1] as u32 & 0x3F) << 6)
            | (bytes[2] as u32 & 0x3F)), 3);
    }
    if b < 0xF5 {
        if bytes.len() < 4 || bytes[1] & 0xC0 != 0x80 || bytes[2] & 0xC0 != 0x80 || bytes[3] & 0xC0 != 0x80 {
            return (None, 1);
        }
        return (Some(((b as u32 & 0x07) << 18)
            | ((bytes[1] as u32 & 0x3F) << 12)
            | ((bytes[2] as u32 & 0x3F) << 6)
            | (bytes[3] as u32 & 0x3F)), 4);
    }
    (None, 1)
}

// ===========================================================================
// 全局控制台
// ===========================================================================

static mut TEXT: TextConsole = TextConsole::new();
static mut FB: FbConsole = FbConsole::new();

/// 在 framebuffer 初始化完成后调用，选择实际使用的控制台并清屏。
pub fn init_display() {
    if crate::fb::available() {
        unsafe {
            (*core::ptr::addr_of_mut!(FB)).init();
        }
        FB_READY.store(true, Ordering::Relaxed);
    } else {
        FB_READY.store(false, Ordering::Relaxed);
        unsafe { (*core::ptr::addr_of_mut!(TEXT)).update_cursor(); }
    }
}

/// 当前控制台尺寸（列、行）。
pub fn size() -> (usize, usize) {
    if FB_READY.load(Ordering::Relaxed) {
        unsafe {
            let c = (*core::ptr::addr_of!(FB)).cols;
            let r = (*core::ptr::addr_of!(FB)).rows;
            (c, r)
        }
    } else {
        (WIDTH, HEIGHT)
    }
}

/// 光标位置（行、列），用于 Shell 重绘输入行。
pub fn cursor_pos() -> (usize, usize) {
    if FB_READY.load(Ordering::Relaxed) {
        unsafe { (*core::ptr::addr_of!(FB)).cursor() }
    } else {
        unsafe {
            let row = (*core::ptr::addr_of!(TEXT)).row;
            let col = (*core::ptr::addr_of!(TEXT)).col;
            (row, col)
        }
    }
}

pub fn set_lang(zh: bool) {
    if zh && !lang_zh() && !crate::fb::available() {
        crate::zhfont::init();
    }
    CHINESE.store(zh, Ordering::Relaxed);
    with_writer(|w| w.clear());
}

/// 所有输出都经过当前活动控制台。
pub fn with_writer<F, R>(f: F) -> R
where
    F: FnOnce(&mut dyn Console) -> R,
{
    if FB_READY.load(Ordering::Relaxed) {
        unsafe { f(&mut *core::ptr::addr_of_mut!(FB)) }
    } else {
        unsafe { f(&mut *core::ptr::addr_of_mut!(TEXT)) }
    }
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {{
        $crate::vga::with_writer(|w| {
            let _ = w.write_fmt(format_args!($($arg)*));
        });
    }};
}

#[macro_export]
macro_rules! println {
    ($($arg:tt)*) => {{
        $crate::vga::with_writer(|w| {
            let _ = w.write_fmt(format_args!($($arg)*));
            let _ = w.write_char('\n');
        });
    }};
    () => {
        $crate::println!("")
    };
}

/// 双语字符串：根据当前语言返回英文或中文。
#[macro_export]
macro_rules! tr {
    ($en:expr, $zh:expr) => {
        if $crate::vga::lang_zh() { $zh } else { $en }
    };
}

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