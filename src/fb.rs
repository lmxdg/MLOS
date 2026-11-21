//! VESA VBE 线性 framebuffer 驱动与像素绘制。
//!
//! 启动引导程序（自研 stage2 / EL Torito 引导镜像）会设置 VBE 图形模式并
//! 在 Multiboot2 信息中填写 framebuffer tag；内核在这里解析真实的分辨率、
//! 扫描线与色深，控制台与 GUI 直接向线性显存绘图。
//!
//! 支持 32bpp（XRGB8888）与 24bpp（BGR888）；如果固件没有提供可用的
//! 线性 framebuffer，则回退到 VGA 文本模式（见 vga.rs）。

use core::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

static PRESENT: AtomicBool = AtomicBool::new(false);
static ADDR: AtomicUsize = AtomicUsize::new(0);
static WIDTH: AtomicU32 = AtomicU32::new(0);
static HEIGHT: AtomicU32 = AtomicU32::new(0);
static PITCH: AtomicU32 = AtomicU32::new(0);
static BPP: AtomicU32 = AtomicU32::new(0);

/// 标准 16 色 VGA 调色板（0x00RRGGBB）。
pub const PALETTE: [u32; 16] = [
    0x000000, 0x0000AA, 0x00AA00, 0x00AAAA,
    0xAA0000, 0xAA00AA, 0xAA5500, 0xAAAAAA,
    0x555555, 0x5555FF, 0x55FF55, 0x55FFFF,
    0xFF5555, 0xFF55FF, 0xFFFF55, 0xFFFFFF,
];

#[inline]
pub const fn rgb(r: u8, g: u8, b: u8) -> u32 {
    ((r as u32) << 16) | ((g as u32) << 8) | b as u32
}

#[inline]
pub fn color(code: u8) -> u32 {
    PALETTE[(code & 0x0F) as usize]
}

/// 解析 Multiboot2 framebuffer tag（type 8）。
///
/// tag 布局：
///   +0  u32 type (=8)
///   +4  u32 size
///   +8  u64 framebuffer_addr
///   +16 u32 framebuffer_pitch
///   +20 u32 framebuffer_width
///   +24 u32 framebuffer_height
///   +28 u8  framebuffer_bpp
///   +29 u8  framebuffer_type (1 = RGB)
///   +30 u16 reserved
///   +32 color info（RGB 时 6 字节）
pub fn init(mbi: u64) {
    PRESENT.store(false, Ordering::Relaxed);
    if mbi == 0 {
        return;
    }
    unsafe {
        let total = *(mbi as *const u32) as usize;
        if total < 16 {
            return;
        }
        let mut off = 8usize;
        while off + 8 <= total {
            let tag = (mbi as *const u8).add(off);
            let tag_type = core::ptr::read_unaligned(tag as *const u32);
            let tag_size = core::ptr::read_unaligned(tag.add(4) as *const u32) as usize;
            if tag_type == 0 || tag_size < 8 {
                break;
            }
            if tag_type == 8 && tag_size >= 32 {
                let addr = core::ptr::read_unaligned(tag.add(8) as *const u64) as usize;
                let pitch = core::ptr::read_unaligned(tag.add(16) as *const u32) as usize;
                let width = core::ptr::read_unaligned(tag.add(20) as *const u32) as usize;
                let height = core::ptr::read_unaligned(tag.add(24) as *const u32) as usize;
                let bpp = *tag.add(28) as usize;
                let ftype = *tag.add(29);
                if ftype == 1 && addr != 0 && width >= 320 && height >= 200 &&
                    pitch >= width * if bpp == 32 { 4 } else { 3 } &&
                    (bpp == 32 || bpp == 24) &&
                    addr.checked_add(pitch * height).map(|end| end <= 0x1_0000_0000).unwrap_or(false)
                {
                    ADDR.store(addr, Ordering::Relaxed);
                    WIDTH.store(width as u32, Ordering::Relaxed);
                    HEIGHT.store(height as u32, Ordering::Relaxed);
                    PITCH.store(pitch as u32, Ordering::Relaxed);
                    BPP.store(bpp as u32, Ordering::Relaxed);
                    PRESENT.store(true, Ordering::Relaxed);
                    return;
                }
            }
            off = (off + tag_size + 7) & !7;
        }
    }
}

#[inline]
pub fn available() -> bool {
    PRESENT.load(Ordering::Relaxed)
}

#[inline]
pub fn width() -> usize {
    WIDTH.load(Ordering::Relaxed) as usize
}

#[inline]
pub fn height() -> usize {
    HEIGHT.load(Ordering::Relaxed) as usize
}

#[inline]
pub fn pitch() -> usize {
    PITCH.load(Ordering::Relaxed) as usize
}

#[inline]
pub fn bpp() -> usize {
    BPP.load(Ordering::Relaxed) as usize
}

#[inline]
pub fn address() -> usize {
    ADDR.load(Ordering::Relaxed)
}

#[inline]
pub fn put_pixel(x: i32, y: i32, color: u32) {
    if !available() {
        return;
    }
    if x < 0 || y < 0 || x as usize >= width() || y as usize >= height() {
        return;
    }
    unsafe {
        let base = address() as *mut u8;
        if bpp() == 32 {
            let p = base.add(y as usize * pitch() + x as usize * 4) as *mut u32;
            core::ptr::write_volatile(p, color);
        } else {
            let p = base.add(y as usize * pitch() + x as usize * 3);
            core::ptr::write_volatile(p, (color & 0xFF) as u8);
            core::ptr::write_volatile(p.add(1), ((color >> 8) & 0xFF) as u8);
            core::ptr::write_volatile(p.add(2), ((color >> 16) & 0xFF) as u8);
        }
    }
}

/// 读取像素颜色（用于鼠标指针下方背景的保存/恢复）。
pub fn get_pixel(x: i32, y: i32) -> u32 {
    if !available() || x < 0 || y < 0 || x as usize >= width() || y as usize >= height() {
        return 0;
    }
    unsafe {
        let base = address() as *const u8;
        if bpp() == 32 {
            let p = base.add(y as usize * pitch() + x as usize * 4) as *const u32;
            core::ptr::read_volatile(p)
        } else {
            let p = base.add(y as usize * pitch() + x as usize * 3);
            (core::ptr::read_volatile(p) as u32)
                | ((core::ptr::read_volatile(p.add(1)) as u32) << 8)
                | ((core::ptr::read_volatile(p.add(2)) as u32) << 16)
        }
    }
}

pub fn fill_rect(x: i32, y: i32, w: i32, h: i32, color: u32) {
    if w <= 0 || h <= 0 || !available() {
        return;
    }
    let x0 = x.max(0);
    let y0 = y.max(0);
    let x1 = (x + w).min(width() as i32);
    let y1 = (y + h).min(height() as i32);
    if x1 <= x0 || y1 <= y0 {
        return;
    }
    for yy in y0..y1 {
        for xx in x0..x1 {
            put_pixel(xx, yy, color);
        }
    }
}

pub fn clear(color: u32) {
    fill_rect(0, 0, width() as i32, height() as i32, color);
}

pub fn draw_hline(x: i32, y: i32, w: i32, color: u32) {
    fill_rect(x, y, w, 1, color);
}

pub fn draw_vline(x: i32, y: i32, h: i32, color: u32) {
    fill_rect(x, y, 1, h, color);
}

pub fn draw_line(x0: i32, y0: i32, x1: i32, y1: i32, color: u32) {
    let dx = (x1 - x0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let dy = -(y1 - y0).abs();
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;
    let (mut x, mut y) = (x0, y0);
    loop {
        put_pixel(x, y, color);
        if x == x1 && y == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
    }
}

/// 绘制一个 8x16 字形，背景不透明。
pub fn draw_ascii(x: i32, y: i32, glyph: &[u8; 16], fg: u32, bg: u32) {
    for row in 0..16 {
        let bits = glyph[row];
        for col in 0..8 {
            let c = if bits & (0x80 >> col) != 0 { fg } else { bg };
            put_pixel(x + col as i32, y + row as i32, c);
        }
    }
}

/// 绘制一个 16x16 汉字/中文标点，背景不透明。
pub fn draw_cjk(x: i32, y: i32, glyph: &[u8; 32], fg: u32, bg: u32) {
    for row in 0..16 {
        let left = glyph[row];
        let right = glyph[row + 16];
        for col in 0..8 {
            let c1 = if left & (0x80 >> col) != 0 { fg } else { bg };
            let c2 = if right & (0x80 >> col) != 0 { fg } else { bg };
            put_pixel(x + col as i32, y + row as i32, c1);
            put_pixel(x + 8 + col as i32, y + row as i32, c2);
        }
    }
}

/// 绘制 UTF-8 文本，支持换行；返回下一字符的 x 坐标。
pub fn draw_text(x: i32, y: i32, s: &str, fg: u32, bg: u32) -> i32 {
    draw_text_clipped(x, y, width() as i32 - x, s, fg, bg)
}

/// 在给定像素宽度内绘制文本；超出宽度时截断（不换行）。
pub fn draw_text_clipped(x: i32, y: i32, max_w: i32, s: &str, fg: u32, bg: u32) -> i32 {
    let mut cx = x;
    let limit = x + max_w.max(0);
    for ch in s.chars() {
        if ch == '\n' {
            cx = x;
            continue;
        }
        if ch == '\r' {
            continue;
        }
        let cp = ch as u32;
        let w = crate::font::char_width(cp) as i32;
        if cx + w > limit {
            break;
        }
        if cp < 0x80 {
            if let Some(g) = crate::font::ascii(cp as u8) {
                draw_ascii(cx, y, g, fg, bg);
            } else {
                draw_ascii(cx, y, crate::font::ascii(b'?').unwrap(), fg, bg);
            }
        } else if let Some(g) = crate::font::cjk(cp) {
            draw_cjk(cx, y, g, fg, bg);
        } else {
            draw_ascii(cx, y, crate::font::ascii(b'?').unwrap(), fg, bg);
            if w > 8 {
                fill_rect(cx + 8, y, 8, 16, bg);
            }
        }
        cx += w;
    }
    cx
}

/// 横向居中绘制文本。
pub fn draw_text_center(y: i32, s: &str, fg: u32, bg: u32) {
    let w = crate::font::text_width(s) as i32;
    draw_text((width() as i32 - w) / 2, y, s, fg, bg);
}

/// 整体上滚 `px` 像素，底部用 `bg` 填充。
pub fn scroll_up(px: usize, bg: u32) {
    if !available() {
        return;
    }
    let w = width();
    let h = height();
    let pitch = pitch();
    let bytes_per_pixel = if bpp() == 32 { 4 } else { 3 };
    let row_bytes = w * bytes_per_pixel;
    unsafe {
        let base = address() as *mut u8;
        for y in px..h {
            let src = base.add(y * pitch);
            let dst = base.add((y - px) * pitch);
            if bpp() == 32 {
                core::ptr::copy(src, dst, row_bytes);
            } else {
                core::ptr::copy(src, dst, row_bytes);
            }
        }
    }
    fill_rect(0, (h - px) as i32, w as i32, px as i32, bg);
}