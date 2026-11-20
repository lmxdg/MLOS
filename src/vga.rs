//! 统一终端层：可自由切换的两种渲染后端
//! - Text: 经典 VGA 文本模式 80x25（英文 ASCII，CP437）
//! - Gfx : VGA 图形模式 320x200x256（16x16 点阵，支持中文 UTF-8）
//!
//! 所有输出统一走 `with_writer` + `print!/println!`，因此各业务模块无需关心当前渲染方式。

use core::fmt;

#[path = "font.rs"]
pub mod font;
#[path = "gfx.rs"]
pub mod gfx;
#[path = "vga_text.rs"]
mod vga_text;

pub use gfx::GfxConsole;
pub use vga_text::VgaTextConsole;

/// 文本模式尺寸（保持对外常量兼容）
pub const WIDTH: usize = vga_text::WIDTH;
pub const HEIGHT: usize = vga_text::HEIGHT;

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

/// 所有渲染后端必须实现的统一终端接口
pub trait Console: fmt::Write {
    fn clear(&mut self);
    fn set_color(&mut self, fg: Color, bg: Color);
    fn put_byte(&mut self, byte: u8);
    fn put_str(&mut self, s: &str);
    fn backspace(&mut self);
    fn update_cursor(&mut self);
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RenderMode {
    Text,
    Gfx,
}

static mut TEXT_CONSOLE: VgaTextConsole = VgaTextConsole::new();
static mut GFX_CONSOLE: GfxConsole = GfxConsole::new();
static mut CURRENT_MODE: RenderMode = RenderMode::Text;

pub fn current_mode() -> RenderMode {
    unsafe { CURRENT_MODE }
}

pub fn is_gfx() -> bool {
    current_mode() == RenderMode::Gfx
}

/// 切换渲染方式：先让硬件进入对应模式，再重置该后端的光标/清屏。
pub fn set_mode(mode: RenderMode) {
    unsafe {
        match mode {
            RenderMode::Text => {
                gfx::set_text_mode();
                CURRENT_MODE = RenderMode::Text;
                TEXT_CONSOLE.reset();
            }
            RenderMode::Gfx => {
                gfx::set_graphics_mode();
                CURRENT_MODE = RenderMode::Gfx;
                GFX_CONSOLE.reset();
            }
        }
    }
}

/// 统一入口：按当前渲染方式把闭包分发给对应后端。
pub fn with_writer<F, R>(f: F) -> R
where
    F: FnOnce(&mut dyn Console) -> R,
{
    unsafe {
        match CURRENT_MODE {
            RenderMode::Text => f(&mut TEXT_CONSOLE),
            RenderMode::Gfx => f(&mut GFX_CONSOLE),
        }
    }
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

/// 双语选择宏：图形模式返回中文串，文本模式返回英文串。
#[macro_export]
macro_rules! tr {
    ($en:expr, $zh:expr) => {
        if $crate::vga::is_gfx() {
            $zh
        } else {
            $en
        }
    };
}

/// 双语 println：按当前渲染模式选择字面量格式串，支持格式化参数。
#[macro_export]
macro_rules! tprintln {
    ($en:literal, $zh:literal $(, $arg:expr)* $(,)?) => {
        if $crate::vga::is_gfx() {
            $crate::println!($zh $(, $arg)*);
        } else {
            $crate::println!($en $(, $arg)*);
        }
    };
}

/// 双语 print：按当前渲染模式选择字面量格式串，支持格式化参数。
#[macro_export]
macro_rules! tprint {
    ($en:literal, $zh:literal $(, $arg:expr)* $(,)?) => {
        if $crate::vga::is_gfx() {
            $crate::print!($zh $(, $arg)*);
        } else {
            $crate::print!($en $(, $arg)*);
        }
    };
}
