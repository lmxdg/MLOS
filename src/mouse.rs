//! PS/2 鼠标驱动（轮询，Stream Mode，3 字节数据包）。
//!
//! 只消费 8042 输出缓冲区中 AUX（鼠标）数据，避免把键盘扫描码误当成
//! 鼠标字节。所有等待都有上界，没有鼠标时初始化会快速失败并回退到键盘。

use crate::fb;
use crate::io::{inb, outb};

const STATUS_PORT: u16 = 0x64;
const DATA_PORT: u16 = 0x60;

static mut X: i32 = 512;
static mut Y: i32 = 384;
static mut LEFT: bool = false;
static mut RIGHT: bool = false;
static mut CLICK: Option<(i32, i32, bool)> = None;
static mut READY: bool = false;
static mut MOVED: bool = false;

#[inline]
fn wait_write() -> bool {
    unsafe {
        for _ in 0..200_000 {
            if inb(STATUS_PORT) & 0x02 == 0 {
                return true;
            }
            core::arch::asm!("pause");
        }
        false
    }
}

/// 等待输出缓冲区有数据；`aux_only` 时要求状态位 5（AUX）置位。
#[inline]
fn wait_read(aux_only: bool, limit: u32) -> bool {
    unsafe {
        for _ in 0..limit {
            let st = inb(STATUS_PORT);
            if st & 0x01 != 0 {
                if !aux_only || st & 0x20 != 0 {
                    return true;
                }
                // 键盘数据：不要吃掉，交给键盘驱动
                return false;
            }
            core::arch::asm!("pause");
        }
        false
    }
}

#[inline]
fn read_data() -> Option<u8> {
    if wait_read(false, 200_000) {
        Some(unsafe { inb(DATA_PORT) })
    } else {
        None
    }
}

#[inline]
fn read_aux() -> Option<u8> {
    if wait_read(true, 200_000) {
        Some(unsafe { inb(DATA_PORT) })
    } else {
        None
    }
}

fn write_ctrl(cmd: u8) -> bool {
    if !wait_write() {
        return false;
    }
    unsafe { outb(STATUS_PORT, cmd); }
    true
}

fn write_mouse(cmd: u8) -> bool {
    if !write_ctrl(0xD4) {
        return false;
    }
    if !wait_write() {
        return false;
    }
    unsafe { outb(DATA_PORT, cmd); }
    true // ACK 由调用者读取，便于 0xFF 复位时的多字节响应
}

fn read_ack() -> bool {
    read_aux() == Some(0xFA)
}

fn wake() {
    if !write_ctrl(0xA8) {
        return;
    }
    let _ = write_ctrl(0x20);
    if let Some(cmb) = read_data() {
        let _ = write_ctrl(0x60);
        let _ = wait_write();
        unsafe { outb(DATA_PORT, cmb | 0x02); }
    }
}

/// 初始化鼠标。返回是否成功启用。
pub fn init() -> bool {
    // 清空遗留的 AUX 字节
    unsafe {
        let mut guard = 0;
        while inb(STATUS_PORT) & 0x21 == 0x21 && guard < 32 {
            let _ = inb(DATA_PORT);
            guard += 1;
        }
    }
    wake();

    // 复位（可选但更可靠）；部分设备复位响应较慢，失败也继续尝试
    if write_mouse(0xFF) {
        let _ = read_ack();
        let _ = read_ack();
        let _ = read_ack();
    }

    let _ack_f6 = write_mouse(0xF6) && read_ack();
    // 采样率 80、分辨率 4、1:1 缩放
    if write_mouse(0xF3) {
        let _ = read_ack();
        if write_mouse(80) {
            let _ = read_ack();
        }
    }
    if write_mouse(0xE8) {
        let _ = read_ack();
        if write_mouse(0x03) {
            let _ = read_ack();
        }
    }
    if write_mouse(0xE6) {
        let _ = read_ack();
    }
    let ok = write_mouse(0xF4) && read_ack();

    unsafe {
        let w = if fb::available() { fb::width() as i32 } else { 640 };
        let h = if fb::available() { fb::height() as i32 } else { 400 };
        X = w / 2;
        Y = h / 2;
        LEFT = false;
        RIGHT = false;
        CLICK = None;
        MOVED = false;
        READY = ok;
    }
    ok
}

#[inline]
pub fn ready() -> bool { unsafe { READY } }

/// 读取一个完整的三字节数据包；所有等待都有上限。
fn poll_packet() -> Option<(i8, i8, u8)> {
    let flags = read_aux()?;
    if flags & 0x08 == 0 {
        return None; // 失步，丢弃
    }
    let dx = read_aux()? as i8;
    let dy = read_aux()? as i8;
    Some((dx, dy, flags))
}

/// 轮询鼠标并更新位置/按键。返回位置是否发生变化。
pub fn update() -> bool {
    if !ready() {
        return false;
    }
    let mut moved = false;
    // 一次 update 最多处理 4 个包，避免高采样率下积压
    for _ in 0..4 {
        let Some((dx, dy, flags)) = poll_packet() else { break; };
        unsafe {
            let w = if fb::available() { fb::width() as i32 } else { 640 };
            let h = if fb::available() { fb::height() as i32 } else { 400 };
            let speed = 2i32;
            if dx != 0 || dy != 0 {
                X = (X + dx as i32 * speed).clamp(0, w - 1);
                Y = (Y - dy as i32 * speed).clamp(0, h - 1);
                moved = true;
            }
            let left = flags & 0x01 != 0;
            let right = flags & 0x02 != 0;
            if left && !LEFT {
                CLICK = Some((X, Y, true));
            } else if right && !RIGHT {
                CLICK = Some((X, Y, false));
            }
            LEFT = left;
            RIGHT = right;
        }
    }
    unsafe { MOVED = moved || MOVED; }
    moved
}

/// 自上次调用以来是否移动过（GUI 用）。
pub fn take_moved() -> bool {
    unsafe {
        let m = MOVED;
        MOVED = false;
        m
    }
}

#[inline]
pub fn x() -> i32 { unsafe { X } }

#[inline]
pub fn y() -> i32 { unsafe { Y } }

#[inline]
pub fn left_down() -> bool { unsafe { LEFT } }

/// 取出一次点击（像素坐标、是否左键），取后清空。
pub fn take_click() -> Option<(i32, i32, bool)> {
    unsafe { CLICK.take() }
}