//! 像素鼠标指针（箭头）。
//!
//! GUI 每次重绘前先 `hide()` 恢复指针下的背景，重绘后 `show()` 再叠加箭头；
//! 仅移动鼠标时只恢复/保存 12x19 的小区域，避免整屏重绘。

use crate::fb;

const W: usize = 12;
const H: usize = 19;
const CX: i32 = -1;
const CY: i32 = -1;

/// 'X' = 黑色描边，'W' = 白色填充，'.' = 透明
const ARROW: [&str; H] = [
    "X...........",
    "XX..........",
    "XWX.........",
    "XWWX........",
    "XWWWX.......",
    "XWWWWX......",
    "XWWWWWX.....",
    "XWWWWWWX....",
    "XWWWWWWWX...",
    "XWWWWWWWWX..",
    "XWWWWWWWWWX.",
    "XWWWWWWXXXX.",
    "XWWWWX......",
    "XWWWXWX.....",
    "XWWX.XWX....",
    "XWX..XWX....",
    "XX...XWX....",
    "X.....XWX...",
    "......XX....",
];

static mut SAVED: [u32; W * H] = [0; W * H];
static mut SAVED_X: i32 = -999;
static mut SAVED_Y: i32 = -999;
static mut VISIBLE: bool = false;
static mut INITIALIZED: bool = false;

fn clamp_pos(x: i32, y: i32) -> (i32, i32) {
    let max_x = (fb::width() as i32 - W as i32).max(0);
    let max_y = (fb::height() as i32 - H as i32).max(0);
    (x.max(0).min(max_x), y.max(0).min(max_y))
}

fn save_and_draw(x: i32, y: i32) {
    unsafe {
        for row in 0..H {
            for col in 0..W {
                SAVED[row * W + col] = fb::get_pixel(x + col as i32, y + row as i32);
            }
        }
        SAVED_X = x;
        SAVED_Y = y;
    }
    for row in 0..H {
        let line = ARROW[row].as_bytes();
        for col in 0..W {
            let c = match line[col] {
                b'X' => Some(fb::rgb(0, 0, 0)),
                b'W' => Some(fb::rgb(255, 255, 255)),
                _ => None,
            };
            if let Some(color) = c {
                fb::put_pixel(x + col as i32, y + row as i32, color);
            }
        }
    }
}

fn restore() {
    unsafe {
        if SAVED_X < -100 || SAVED_Y < -100 {
            return;
        }
        for row in 0..H {
            for col in 0..W {
                fb::put_pixel(SAVED_X + col as i32, SAVED_Y + row as i32, SAVED[row * W + col]);
            }
        }
        SAVED_X = -999;
        SAVED_Y = -999;
    }
}

pub fn init() {
    if !fb::available() {
        return;
    }
    unsafe { INITIALIZED = true; }
    move_to(crate::mouse::x(), crate::mouse::y());
}

/// 隐藏指针并恢复背景。
pub fn hide() {
    unsafe {
        if VISIBLE {
            restore();
            VISIBLE = false;
        }
    }
}

/// 显示指针（保存当前背景）。
pub fn show() {
    unsafe {
        if !INITIALIZED || VISIBLE {
            return;
        }
        let (x, y) = clamp_pos(crate::mouse::x() + CX, crate::mouse::y() + CY);
        save_and_draw(x, y);
        VISIBLE = true;
    }
}

/// 仅移动指针：恢复旧背景，保存新背景并绘制。
pub fn sync() {
    unsafe {
        if !INITIALIZED {
            return;
        }
        let (x, y) = clamp_pos(crate::mouse::x() + CX, crate::mouse::y() + CY);
        if VISIBLE && x == SAVED_X && y == SAVED_Y {
            return;
        }
        if VISIBLE {
            restore();
            VISIBLE = false;
        }
        save_and_draw(x, y);
        VISIBLE = true;
    }
}

pub fn move_to(x: i32, y: i32) {
    let _ = x;
    let _ = y;
    sync();
}
