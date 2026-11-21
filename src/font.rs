//! 位图字体：8x16 ASCII + 16x16 中文点阵。
//!
//! framebuffer 控制台与 GUI 都直接使用这里的字形数据，不再改写 VGA 字模，
//! 因此不会出现字形槽被覆盖导致的乱码。

mod ascii_data {
    include!("ascii_font_data.rs");
}

mod cjk_data {
    include!("zhfont_data.rs");
}

pub const ASCII_W: usize = 8;
pub const CJK_W: usize = 16;
pub const CELL_H: usize = 16;

/// 8x16 ASCII 字形（0x20..0x7E）。
pub fn ascii(c: u8) -> Option<&'static [u8; 16]> {
    if c < ascii_data::FIRST || c > ascii_data::LAST {
        return None;
    }
    ascii_data::ASCII_GLYPHS.get((c - ascii_data::FIRST) as usize)
}

/// 16x16 汉字/中文标点字形（左 8 列在前）。
pub fn cjk(cp: u32) -> Option<&'static [u8; 32]> {
    cjk_data::glyph(cp)
}

/// 该字符是否按双宽（16px）显示。
pub fn is_wide(cp: u32) -> bool {
    matches!(cp,
        0x1100..=0x115F | 0x2E80..=0xA4CF | 0xAC00..=0xD7A3 |
        0xF900..=0xFAFF | 0xFE30..=0xFE4F | 0xFF00..=0xFF60 |
        0xFFE0..=0xFFE6)
}

/// 字符在屏幕上的像素宽度（ASCII 8px，双宽字符 16px，其他按 8px）。
pub fn char_width(cp: u32) -> usize {
    if cp < 0x80 {
        ASCII_W
    } else if is_wide(cp) || cjk(cp).is_some() {
        CJK_W
    } else {
        ASCII_W
    }
}

/// 字符串在屏幕上的像素宽度（按 UTF-8 解码，忽略换行）。
pub fn text_width(s: &str) -> usize {
    let mut w = 0usize;
    for ch in s.chars() {
        if ch == '\n' || ch == '\r' {
            continue;
        }
        w += char_width(ch as u32);
    }
    w
}