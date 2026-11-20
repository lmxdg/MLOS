//! PS/2 键盘驱动（轮询方式，Scancode Set 1）

use crate::io::inb;

const DATA_PORT: u16 = 0x60;
const STATUS_PORT: u16 = 0x64;

#[derive(Clone, Copy)]
pub enum Key {
    Char(char),
    Backspace,
    Enter,
    Tab,
}

pub struct Keyboard {
    shift: bool,
    caps_lock: bool,
}

impl Keyboard {
    pub const fn new() -> Self {
        Self { shift: false, caps_lock: false }
    }

    pub fn handle_scancode(&mut self, scancode: u8) -> Option<Key> {
        let released = scancode & 0x80 != 0;
        let code = scancode & 0x7F;

        match code {
            0x2A | 0x36 => {
                self.shift = !released;
                return None;
            }
            0x3A => {
                if !released {
                    self.caps_lock = !self.caps_lock;
                }
                return None;
            }
            _ => {}
        }
        if released {
            return None;
        }

        let upper = self.shift ^ self.caps_lock;
        let ch = match code {
            0x02 => pick('1', '!', self.shift),
            0x03 => pick('2', '@', self.shift),
            0x04 => pick('3', '#', self.shift),
            0x05 => pick('4', '$', self.shift),
            0x06 => pick('5', '%', self.shift),
            0x07 => pick('6', '^', self.shift),
            0x08 => pick('7', '&', self.shift),
            0x09 => pick('8', '*', self.shift),
            0x0A => pick('9', '(', self.shift),
            0x0B => pick('0', ')', self.shift),
            0x0C => pick('-', '_', self.shift),
            0x0D => pick('=', '+', self.shift),
            0x0E => return Some(Key::Backspace),
            0x0F => return Some(Key::Tab),
            0x10 => letter('q', upper),
            0x11 => letter('w', upper),
            0x12 => letter('e', upper),
            0x13 => letter('r', upper),
            0x14 => letter('t', upper),
            0x15 => letter('y', upper),
            0x16 => letter('u', upper),
            0x17 => letter('i', upper),
            0x18 => letter('o', upper),
            0x19 => letter('p', upper),
            0x1A => pick('[', '{', self.shift),
            0x1B => pick(']', '}', self.shift),
            0x1C => return Some(Key::Enter),
            0x1E => letter('a', upper),
            0x1F => letter('s', upper),
            0x20 => letter('d', upper),
            0x21 => letter('f', upper),
            0x22 => letter('g', upper),
            0x23 => letter('h', upper),
            0x24 => letter('j', upper),
            0x25 => letter('k', upper),
            0x26 => letter('l', upper),
            0x27 => pick(';', ':', self.shift),
            0x28 => pick('\'', '"', self.shift),
            0x29 => pick('`', '~', self.shift),
            0x2B => pick('\\', '|', self.shift),
            0x2C => letter('z', upper),
            0x2D => letter('x', upper),
            0x2E => letter('c', upper),
            0x2F => letter('v', upper),
            0x30 => letter('b', upper),
            0x31 => letter('n', upper),
            0x32 => letter('m', upper),
            0x33 => pick(',', '<', self.shift),
            0x34 => pick('.', '>', self.shift),
            0x35 => pick('/', '?', self.shift),
            0x39 => ' ',
            _ => return None,
        };
        Some(Key::Char(ch))
    }
}

#[inline(always)]
fn pick(normal: char, shifted: char, shift: bool) -> char {
    if shift { shifted } else { normal }
}

#[inline(always)]
fn letter(base: char, upper: bool) -> char {
    if upper { base.to_ascii_uppercase() } else { base }
}

pub fn read_scancode() -> Option<u8> {
    unsafe {
        if inb(STATUS_PORT) & 1 != 0 {
            Some(inb(DATA_PORT))
        } else {
            None
        }
    }
}

pub fn controller_busy() -> bool {
    unsafe { inb(STATUS_PORT) & 0x02 != 0 }
}
