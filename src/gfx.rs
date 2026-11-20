//! 渲染后端二：VGA 图形模式 0x13（320x200x256 色）+ 16x16 点阵中文
//!
//! 通过直接写 VGA 寄存器进入线性帧缓冲模式，帧缓冲位于物理地址 0xA0000，
//! 每字节一个像素（调色板索引）。字符以 16x16 点阵绘制，UTF-8 解码后
//! 查 font 字库，因此可显示中文。

use crate::io::{inb, outb};
use crate::vga::{font, Color, Console};
use core::fmt;

pub const FB_ADDR: usize = 0xA0000;
pub const FB_WIDTH: usize = 320;
pub const FB_HEIGHT: usize = 200;
pub const CELL: usize = 16;
pub const COLS: usize = FB_WIDTH / CELL; // 20 列
pub const ROWS: usize = FB_HEIGHT / CELL; // 12 行

// ---- 模式 13h（320x200x256，70Hz）标准寄存器表 ----
const M_GFX: u8 = 0x63;
const SEQ_GFX: [u8; 5] = [0x03, 0x01, 0x0F, 0x00, 0x0E];
const CRT_GFX: [u8; 24] = [
    0x5F, 0x4F, 0x50, 0x82, 0x54, 0x80, 0xBF, 0x1F,
    0x00, 0x41, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x9C, 0x8E, 0x8F, 0x28, 0x40, 0x96, 0xB9, 0xA3,
];
const GC_GFX: [u8; 9] = [0x00, 0x00, 0x00, 0x00, 0x00, 0x40, 0x05, 0x0F, 0xFF];
const ATTR_GFX: [u8; 21] = [
    0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07,
    0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F,
    0x41, 0x00, 0x0F, 0x00, 0x00,
];

// ---- 模式 03h（80x25 文本）标准寄存器表 ----
const M_TEXT: u8 = 0x67;
const SEQ_TEXT: [u8; 5] = [0x03, 0x00, 0x0F, 0x00, 0x07];
const CRT_TEXT: [u8; 24] = [
    0x5F, 0x4F, 0x50, 0x82, 0x55, 0x81, 0xBF, 0x1F,
    0x00, 0x4F, 0x0C, 0x0D, 0x00, 0x00, 0x00, 0x00,
    0x9C, 0x8E, 0x8F, 0x28, 0x1F, 0x96, 0xB9, 0xA3,
];
const GC_TEXT: [u8; 9] = [0x00, 0x00, 0x00, 0x00, 0x00, 0x10, 0x0E, 0x0F, 0xFF];
const ATTR_TEXT: [u8; 21] = [
    0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07,
    0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F,
    0x0C, 0x00, 0x0F, 0x08, 0x00,
];

/// 调色板：索引 0~15 使用标准 VGA 16 色（6bit/通道）。
unsafe fn set_palette() {
    const COLORS: [[u8; 3]; 16] = [
        [0, 0, 0],     [0, 0, 42],    [0, 42, 0],    [0, 42, 42],
        [42, 0, 0],    [42, 0, 42],   [42, 42, 0],   [42, 42, 42],
        [21, 21, 21],  [21, 21, 63],  [21, 63, 21],  [21, 63, 63],
        [63, 21, 21],  [63, 21, 63],  [63, 63, 21],  [63, 63, 63],
    ];
    outb(0x3C8, 0);
    for c in COLORS.iter() {
        outb(0x3C9, c[0]);
        outb(0x3C9, c[1]);
        outb(0x3C9, c[2]);
    }
}

/// 一次性写入一整套 VGA 寄存器配置。
unsafe fn apply_mode(misc: u8, seq: &[u8; 5], crtc: &[u8; 24], gc: &[u8; 9], attr: &[u8; 21]) {
    // 1) 关闭显示，避免切换瞬间花屏
    outb(0x3C4, 0x01);
    let s = inb(0x3C5);
    outb(0x3C5, s | 0x20);

    // 2) 杂项输出
    outb(0x3C2, misc);

    // 3) 解锁 CRTC（允许写 0x00~0x0F）
    outb(0x3D4, 0x04);
    outb(0x3D5, 0x30);
    outb(0x3D4, 0x05);
    outb(0x3D5, 0xE3);

    // 4) 时序发生器（保持关闭显示位）
    for i in 0..5 {
        if i == 1 {
            continue;
        }
        outb(0x3C4, i as u8);
        outb(0x3C5, seq[i]);
    }

    // 5) CRTC：0x11 含写保护位，必须最后写，否则后面的 4/5 会被保护拒绝。
    //    index 5 也带保护位，先写清保护的值，最后再恢复。
    for i in 0..24 {
        if i == 0x11 {
            continue;
        }
        let val = if i == 5 { crtc[i] & 0x7F } else { crtc[i] };
        outb(0x3D4, i as u8);
        outb(0x3D5, val);
    }
    // 最后写 0x11（Vertical Retrace End，含写保护位）与 index 5 的终值
    outb(0x3D4, 0x11);
    outb(0x3D5, crtc[0x11]);
    outb(0x3D4, 0x05);
    outb(0x3D5, crtc[5]);

    // 6) 图形控制器
    for i in 0..9 {
        outb(0x3CE, i as u8);
        outb(0x3CF, gc[i]);
    }

    // 7) 属性控制器（读 0x3DA 复位 index/data 翻转触发器）
    let _ = inb(0x3DA);
    for i in 0..21 {
        outb(0x3C0, i as u8);
        outb(0x3C0, attr[i]);
    }
    let _ = inb(0x3DA);
    outb(0x3C0, 0x20); // PAL=0，开启显示

    // 8) 恢复显示开启
    outb(0x3C4, 0x01);
    outb(0x3C5, seq[1]);
}

// ---- VGA 寄存器快照：进入图形前保存文本模式状态，切回时原样恢复 ----
struct VgaState {
    misc: u8,
    seq: [u8; 5],
    crtc: [u8; 24],
    gc: [u8; 9],
    attr: [u8; 21],
}

static mut SAVED: Option<VgaState> = None;

unsafe fn read_state() -> VgaState {
    let mut st = VgaState {
        misc: inb(0x3CC),
        seq: [0; 5],
        crtc: [0; 24],
        gc: [0; 9],
        attr: [0; 21],
    };
    // sequencer
    for i in 0..5 {
        outb(0x3C4, i as u8);
        st.seq[i] = inb(0x3C5);
    }
    // 解锁 CRTC 后再读（index 5 的保护位先清掉）
    outb(0x3D4, 0x04);
    outb(0x3D5, 0x30);
    outb(0x3D4, 0x05);
    outb(0x3D5, 0xE3);
    for i in 0..24 {
        outb(0x3D4, i as u8);
        st.crtc[i] = inb(0x3D5);
    }
    // graphics controller
    for i in 0..9 {
        outb(0x3CE, i as u8);
        st.gc[i] = inb(0x3CF);
    }
    // attribute controller（读 0x3DA 清 flip-flop，index 从 0x3C0 写、数据从 0x3C1 读）
    let _ = inb(0x3DA);
    for i in 0..21 {
        outb(0x3C0, i as u8);
        st.attr[i] = inb(0x3C1);
    }
    let _ = inb(0x3DA);
    outb(0x3C0, 0x20);
    st
}

/// 用 apply_mode 无法可靠还原 BIOS/GRUB 的初始文本模式，因此直接把保存的
/// 寄存器逐字节写回，保证切回后与原文本模式完全一致。
unsafe fn restore_state(st: &VgaState) {
    outb(0x3C4, 0x01);
    let s = inb(0x3C5);
    outb(0x3C5, s | 0x20); // 关显示
    outb(0x3C2, st.misc);
    // sequencer（跳过 index1，最后统一打开显示）
    for i in 0..5 {
        if i == 1 {
            continue;
        }
        outb(0x3C4, i as u8);
        outb(0x3C5, st.seq[i]);
    }
    // CRTC：0x11 保护位最后写
    outb(0x3D4, 0x04);
    outb(0x3D5, 0x30);
    outb(0x3D4, 0x05);
    outb(0x3D5, st.crtc[5] & 0x7F);
    for i in 0..24 {
        if i == 0x11 || i == 4 || i == 5 {
            continue;
        }
        outb(0x3D4, i as u8);
        outb(0x3D5, st.crtc[i]);
    }
    outb(0x3D4, 0x04);
    outb(0x3D5, st.crtc[4]);
    outb(0x3D4, 0x11);
    outb(0x3D5, st.crtc[0x11]);
    outb(0x3D4, 0x05);
    outb(0x3D5, st.crtc[5]);
    // graphics controller
    for i in 0..9 {
        outb(0x3CE, i as u8);
        outb(0x3CF, st.gc[i]);
    }
    // attribute controller
    let _ = inb(0x3DA);
    for i in 0..21 {
        outb(0x3C0, i as u8);
        outb(0x3C0, st.attr[i]);
    }
    let _ = inb(0x3DA);
    outb(0x3C0, 0x20);
    // 打开显示
    outb(0x3C4, 0x01);
    outb(0x3C5, st.seq[1]);
}

/// 进入图形模式（中文渲染）。进入前保存当前文本模式寄存器快照。
pub fn set_graphics_mode() {
    unsafe {
        if SAVED.is_none() {
            SAVED = Some(read_state());
        }
        apply_mode(M_GFX, &SEQ_GFX, &CRT_GFX, &GC_GFX, &ATTR_GFX);
        set_palette();
    }
}

/// 回到文本模式（英文渲染）。若曾进入图形模式则精确还原快照。
pub fn set_text_mode() {
    unsafe {
        if let Some(ref st) = SAVED {
            restore_state(st);
        }
    }
}

// ---------------- 图形终端 ----------------

pub struct GfxConsole {
    row: usize,
    col: usize,
    fg: u8,
    bg: u8,
    utf_buf: [u8; 4],
    utf_len: usize,
    utf_need: usize,
}

impl GfxConsole {
    pub const fn new() -> Self {
        Self {
            row: 0,
            col: 0,
            fg: 15,
            bg: 0,
            utf_buf: [0; 4],
            utf_len: 0,
            utf_need: 0,
        }
    }

    pub fn reset(&mut self) {
        self.row = 0;
        self.col = 0;
        self.utf_len = 0;
        self.utf_need = 0;
        self.clear();
    }

    pub fn clear(&mut self) {
        let bg = self.bg;
        unsafe {
            let fb = FB_ADDR as *mut u8;
            for i in 0..(FB_WIDTH * FB_HEIGHT) {
                core::ptr::write_volatile(fb.add(i), bg);
            }
        }
        self.row = 0;
        self.col = 0;
    }

    pub fn set_color(&mut self, fg: Color, bg: Color) {
        self.fg = fg.code();
        self.bg = bg.code();
    }

    unsafe fn put_pixel(&self, x: usize, y: usize, color: u8) {
        if x < FB_WIDTH && y < FB_HEIGHT {
            let fb = FB_ADDR as *mut u8;
            core::ptr::write_volatile(fb.add(y * FB_WIDTH + x), color);
        }
    }

    fn fill_cell(&mut self, col: usize, row: usize) {
        let x0 = col * CELL;
        let y0 = row * CELL;
        unsafe {
            for dy in 0..CELL {
                for dx in 0..CELL {
                    self.put_pixel(x0 + dx, y0 + dy, self.bg);
                }
            }
        }
    }

    fn draw_glyph(&mut self, cp: u32) {
        let glyph = match font::lookup(cp) {
            Some(g) => g,
            None => {
                self.fill_cell(self.col, self.row);
                self.col += 1;
                if self.col >= COLS {
                    self.newline();
                }
                return;
            }
        };
        let x0 = self.col * CELL;
        let y0 = self.row * CELL;
        let fg = self.fg;
        let bg = self.bg;
        unsafe {
            for (dy, &bits) in glyph.rows.iter().enumerate() {
                for dx in 0..CELL {
                    let color = if bits & (1 << (15 - dx)) != 0 { fg } else { bg };
                    self.put_pixel(x0 + dx, y0 + dy, color);
                }
            }
        }
        self.col += 1;
        if self.col >= COLS {
            self.newline();
        }
    }

    fn newline(&mut self) {
        self.col = 0;
        self.row += 1;
        if self.row >= ROWS {
            self.scroll();
        }
    }

    fn scroll(&mut self) {
        unsafe {
            let fb = FB_ADDR as *mut u8;
            for y in CELL..(ROWS * CELL) {
                for x in 0..FB_WIDTH {
                    let v = core::ptr::read_volatile(fb.add(y * FB_WIDTH + x));
                    core::ptr::write_volatile(fb.add((y - CELL) * FB_WIDTH + x), v);
                }
            }
            let bg = self.bg;
            let y0 = (ROWS - 1) * CELL;
            for y in y0..(ROWS * CELL) {
                for x in 0..FB_WIDTH {
                    core::ptr::write_volatile(fb.add(y * FB_WIDTH + x), bg);
                }
            }
        }
        self.row = ROWS - 1;
    }

    pub fn backspace(&mut self) {
        if self.col > 0 {
            self.col -= 1;
            self.fill_cell(self.col, self.row);
        }
    }

    /// 图形模式无硬件光标，字符本身就是反馈，这里留空。
    pub fn update_cursor(&self) {}

    pub fn put_byte(&mut self, byte: u8) {
        if self.utf_need == 0 {
            match byte {
                b'\n' => {
                    self.newline();
                    return;
                }
                b'\r' => {
                    self.col = 0;
                    return;
                }
                0x08 => {
                    self.backspace();
                    return;
                }
                b'\t' => {
                    self.col = (self.col / 4 + 1) * 4;
                    if self.col >= COLS {
                        self.newline();
                    }
                    return;
                }
                0x20..=0x7E => {
                    self.draw_glyph(byte as u32);
                    return;
                }
                0xC2..=0xDF => {
                    self.utf_buf[0] = byte;
                    self.utf_len = 1;
                    self.utf_need = 2;
                    return;
                }
                0xE0..=0xEF => {
                    self.utf_buf[0] = byte;
                    self.utf_len = 1;
                    self.utf_need = 3;
                    return;
                }
                0xF0..=0xF4 => {
                    self.utf_buf[0] = byte;
                    self.utf_len = 1;
                    self.utf_need = 4;
                    return;
                }
                _ => return,
            }
        }
        if self.utf_len < 4 {
            self.utf_buf[self.utf_len] = byte;
            self.utf_len += 1;
            if self.utf_len == self.utf_need {
                let cp = decode_utf8(&self.utf_buf[..self.utf_len]);
                self.utf_need = 0;
                self.utf_len = 0;
                if let Some(cp) = cp {
                    self.draw_glyph(cp);
                }
            }
        } else {
            self.utf_need = 0;
            self.utf_len = 0;
        }
    }

    pub fn put_str(&mut self, s: &str) {
        for &b in s.as_bytes() {
            self.put_byte(b);
        }
    }
}

fn decode_utf8(b: &[u8]) -> Option<u32> {
    match b.len() {
        1 => Some(b[0] as u32),
        2 => Some((((b[0] & 0x1F) as u32) << 6) | ((b[1] & 0x3F) as u32)),
        3 => Some(
            (((b[0] & 0x0F) as u32) << 12)
                | (((b[1] & 0x3F) as u32) << 6)
                | ((b[2] & 0x3F) as u32),
        ),
        4 => Some(
            (((b[0] & 0x07) as u32) << 18)
                | (((b[1] & 0x3F) as u32) << 12)
                | (((b[2] & 0x3F) as u32) << 6)
                | ((b[3] & 0x3F) as u32),
        ),
        _ => None,
    }
}

impl Console for GfxConsole {
    fn clear(&mut self) {
        GfxConsole::clear(self);
    }
    fn set_color(&mut self, fg: Color, bg: Color) {
        GfxConsole::set_color(self, fg, bg);
    }
    fn put_byte(&mut self, byte: u8) {
        GfxConsole::put_byte(self, byte);
    }
    fn put_str(&mut self, s: &str) {
        GfxConsole::put_str(self, s);
    }
    fn backspace(&mut self) {
        GfxConsole::backspace(self);
    }
    fn update_cursor(&mut self) {
        GfxConsole::update_cursor(self);
    }
}

impl fmt::Write for GfxConsole {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.put_str(s);
        Ok(())
    }
}
