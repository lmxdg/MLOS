//! 通用行输入读取（供 Shell、安装向导、游戏共用）

use crate::keyboard::{self, Key, Keyboard};
use crate::vga;

/// 读取一行输入，回显字符到屏幕。
/// `mask` 为 Some(c) 时，把输入字符替换为 c 显示（用于密码输入）。
/// 返回实际读取的字节数。
pub fn read_line(buf: &mut [u8], mask: Option<u8>) -> usize {
    let mut kb = Keyboard::new();
    let mut len = 0usize;
    let cap = buf.len();

    loop {
        if let Some(sc) = keyboard::read_scancode() {
            if let Some(key) = kb.handle_scancode(sc) {
                match key {
                    Key::Char(ch) if ch.is_ascii() => {
                        if len < cap - 1 {
                            buf[len] = ch as u8;
                            len += 1;
                            let display = mask.unwrap_or(ch as u8);
                            vga::with_writer(|w| w.put_byte(display));
                        }
                    }
                    Key::Backspace => {
                        if len > 0 {
                            len -= 1;
                            vga::with_writer(|w| w.backspace());
                        }
                    }
                    Key::Enter => {
                        vga::with_writer(|w| w.put_byte(b'\n'));
                        buf[len] = 0;
                        return len;
                    }
                    _ => {}
                }
            }
        }
        unsafe { core::arch::asm!("pause"); }
    }
}

/// 等待用户按任意键
pub fn wait_key() {
    loop {
        if keyboard::read_scancode().is_some() {
            return;
        }
        unsafe { core::arch::asm!("pause"); }
    }
}
