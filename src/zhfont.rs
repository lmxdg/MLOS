//! 中文（预览版）显示支持 —— VGA 字符发生器字模重编程。
//!
//! MLOS 运行在 VGA 文本模式（80x25），显存里每个字符只是一个字节 + 属性。
//! 要显示汉字，我们把字符码 0x80..0xFF 对应的字模替换为汉字点阵：每个
//! 16x16 汉字拆成左右两个 8x16 字形，占用两个连续字符位。ASCII 区
//! (0x00..0x7F) 保持 ROM 默认字模不变，因此英文、命令行、GUI 边框不受影响。
//!
//! 预览版限制：
//!   - 字形槽只有 64 个（128 个半字模），同屏不同汉字超过 64 个时，最早
//!     装入的字模会被覆盖；滚动后旧内容自然消失，通常不影响阅读。
//!   - 字库仅收录系统文案中出现的汉字（由 tools/gen_zhfont.py 生成），
//!     未收录的字符降级显示为 '?'。

use crate::io::{inb, outb};

mod data {
    include!("zhfont_data.rs");
}

/// 可用的汉字槽数量（每槽显示 1 个汉字 = 2 个字符位）
const SLOT_COUNT: usize = 64;
/// 第一个用于汉字半字形的字符码
const BASE_CODE: u8 = 0x80;
const VRAM_FONT: usize = 0xB8000;

static mut SLOTS: [u32; SLOT_COUNT] = [0; SLOT_COUNT];
static mut NEXT_SLOT: usize = 0;

#[inline]
fn seq_read(idx: u8) -> u8 {
    unsafe {
        outb(0x3C4, idx);
        inb(0x3C5)
    }
}

#[inline]
fn seq_write(idx: u8, val: u8) {
    unsafe {
        outb(0x3C4, idx);
        outb(0x3C5, val);
    }
}

#[inline]
fn gfx_read(idx: u8) -> u8 {
    unsafe {
        outb(0x3CE, idx);
        inb(0x3CF)
    }
}

#[inline]
fn gfx_write(idx: u8, val: u8) {
    unsafe {
        outb(0x3CE, idx);
        outb(0x3CF, val);
    }
}

/// 配置 VGA 控制器进入「字模 RAM 写入」状态。
/// 参考 Linux vgacon / OSDev 的标准序列。返回值用于之后恢复。
unsafe fn font_write_enter() -> (u8, u8, u8, u8) {
    let s2 = seq_read(2); // Map Mask
    let s4 = seq_read(4); // Memory Mode
    let g5 = gfx_read(5); // Graphics Mode
    let g6 = gfx_read(6); // Miscellaneous

    // Sequencer Map Mask: 只允许写 plane 2（字符发生器 RAM）
    seq_write(2, 0x04);
    // Sequencer Memory Mode: bit1=1 -> odd/even 关闭；bit2=0 -> 保持 alpha(文本)模式。
    // 若 bit2=1 会退出文本模式，导致字模写入落在错误区域（Linux vgacon 用 0x02）。
    seq_write(4, 0x02);
    // Graphics Controller Graphics Mode register: write mode 0
    gfx_write(5, 0x00);
    // Graphics Controller Miscellaneous: bit2=1 -> host odd/even 关闭，
    // CPU 地址线性映射到 plane 2；bit0=0 保持文本模式。
    gfx_write(6, 0x04);
    (s2, s4, g5, g6)
}

unsafe fn font_write_leave(state: (u8, u8, u8, u8)) {
    seq_write(2, state.0);
    seq_write(4, state.1);
    gfx_write(5, state.2);
    gfx_write(6, state.3);
}

/// 把一个 8x16 点阵上传到指定字符码的字模槽。
unsafe fn upload_glyph(code: u8, rows: &[u8]) {
    let state = font_write_enter();

    // 字模 RAM 按 32 字节槽组织（兼容 8x32 字体），8x16 字体占每槽前 16 字节。
    // 字符码 code 的字形位于物理偏移 code*32（已由 'A' 重编程实验验证生效）。
    // 不同 VGA 实现（真硬件 / QEMU std VGA）对字符码 >=0x80 使用不同字模 bank：
    // 有的用槽 code，有的用槽 code+0x100。两处都写以确保兼容。
    // 两种可能的 Map B 物理映射都写：code（bank0 同索引）与
    // code+0x100（第二字模区），覆盖不同 VGA 实现。
    for &slot in &[code as usize, code as usize + 0x100] {
        let dst = (VRAM_FONT as *mut u8).add(slot * 32);
        for i in 0..32 {
            let v = if i < 16 { rows[i] } else { 0 };
            core::ptr::write_volatile(dst.add(i), v);
        }
    }

    font_write_leave(state);
}

/// 初始化：把最高频的 64 个汉字预装入字模槽。
pub fn init() {
    unsafe {
        // Character Map Select（Sequencer reg 3）：
        //   bits 0-1 = Map A（字符码 0x00-0x7F 的字模库选择）
        //   bits 2-3 = Map B（字符码 0x80-0xFF 的字模库选择）
        // 两者都选 bank 0，让 0x80-0xFF 使用我们重编程的字模。
        seq_write(3, 0x00);

        for (slot, &cp) in data::ZH_HOT.iter().enumerate() {
            if let Some(idx) = data::index_of(cp) {
                load_into(slot, &data::ZH_GLYPHS[idx]);
            }
            SLOTS[slot] = cp;
        }
        NEXT_SLOT = 0;
    }
}

unsafe fn load_into(slot: usize, glyph: &[u8; 32]) {
    let base = BASE_CODE.wrapping_add((slot as u8).wrapping_mul(2));
    upload_glyph(base, &glyph[..16]);
    upload_glyph(base.wrapping_add(1), &glyph[16..]);
}

/// 确保汉字的字模已装入某个槽，返回它在屏幕上应写的两个字符码。
pub fn glyph_bytes(cp: u32) -> Option<(u8, u8)> {
    unsafe {
        for slot in 0..SLOT_COUNT {
            if SLOTS[slot] == cp {
                return Some(slot_codes(slot));
            }
        }
        let idx = data::index_of(cp)?;
        // 槽位轮转分配：优先空槽，然后覆盖最早的槽。
        let slot = NEXT_SLOT % SLOT_COUNT;
        NEXT_SLOT = NEXT_SLOT.wrapping_add(1);
        load_into(slot, &data::ZH_GLYPHS[idx]);
        SLOTS[slot] = cp;
        Some(slot_codes(slot))
    }
}

#[inline]
fn slot_codes(slot: usize) -> (u8, u8) {
    let base = BASE_CODE.wrapping_add((slot as u8).wrapping_mul(2));
    (base, base.wrapping_add(1))
}
