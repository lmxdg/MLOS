//! PS/2 鼠标驱动（轮询方式，Stream Mode，3 字节数据包）。
//!
//! 通过 8042 键盘控制器与鼠标通信（端口 0x60/0x64）。驱动维护鼠标在
//! 文本屏幕上的像素坐标（字符单元 8x16），并把像素坐标换算成 80x25
//! 字符网格的行列，供 GUI 做命中检测。
//!
//! 数据包格式（3 字节）：
//!   byte0: bit0 左键, bit1 右键, bit2 中键, bit3 固定 1,
//!          bit4 X 符号, bit5 Y 符号, bit6 X 溢出, bit7 Y 溢出
//!   byte1: X 相对位移（有符号）
//!   byte2: Y 相对位移（有符号，向上为正）

use crate::io::{inb, outb};
use crate::vga;

const STATUS_PORT: u16 = 0x64;
const DATA_PORT: u16 = 0x60;
const CELL_W: i32 = 8;
const CELL_H: i32 = 16;

static mut X: i32 = 40 * CELL_W;
static mut Y: i32 = 12 * CELL_H;
static mut LEFT: bool = false;
static mut RIGHT: bool = false;
static mut CLICK: Option<(usize, usize, bool)> = None;
static mut READY: bool = false;

#[inline]
fn wait_write() {
    unsafe {
        while inb(STATUS_PORT) & 0x02 != 0 {
            core::arch::asm!("pause");
        }
    }
}

#[inline]
fn wait_read() {
    unsafe {
        while inb(STATUS_PORT) & 0x01 == 0 {
            core::arch::asm!("pause");
        }
    }
}

#[inline]
fn read_byte() -> u8 {
    wait_read();
    unsafe { inb(DATA_PORT) }
}

fn write_ctrl(cmd: u8) {
    wait_write();
    unsafe { outb(STATUS_PORT, cmd); }
}

fn write_mouse(cmd: u8) {
    wait_write();
    unsafe { outb(STATUS_PORT, 0xD4); }
    wait_write();
    unsafe { outb(DATA_PORT, cmd); }
    let _ = read_byte(); // ACK (0xFA)
}

/// 初始化鼠标。返回是否成功启用。
pub fn init() -> bool {
    unsafe {
        // 1. 启用辅助设备（鼠标）
        write_ctrl(0xA8);
        // 2. 读取控制器配置字节，打开鼠标中断位（无中断也需启用设备）
        write_ctrl(0x20);
        let cmb = read_byte();
        write_ctrl(0x60);
        wait_write();
        outb(DATA_PORT, cmb | 0x02);
        // 3. 鼠标默认值 + 启用数据上报
        write_mouse(0xF6); // Set Defaults
        write_mouse(0xF4); // Enable Reporting
        // 4. 丢弃上电后可能残留的数据包字节
        for _ in 0..3 {
            if inb(STATUS_PORT) & 0x01 != 0 {
                let _ = inb(DATA_PORT);
            }
        }
        X = 40 * CELL_W;
        Y = 12 * CELL_H;
        LEFT = false;
        RIGHT = false;
        CLICK = None;
        READY = true;
        true
    }
}

#[inline]
pub fn ready() -> bool {
    unsafe { READY }
}

/// 读取一个完整的数据包。非阻塞：只有 3 个字节都已到达才读取一整包，
/// 避免因部分到达而在 read_byte() 上永久阻塞（会导致 GUI 卡死）。
/// 返回 (dx, dy, flags)。
fn poll_packet() -> Option<(i8, i8, u8)> {
    unsafe {
        // 3 字节包必须全部就绪才处理，否则等下一次轮询。
        if inb(STATUS_PORT) & 0x01 == 0 {
            return None;
        }
        let flags = inb(DATA_PORT);
        if flags & 0x08 == 0 {
            // 失去同步：丢弃这个字节，等待下一个包起始（bit3 应为 1）
            return None;
        }
        // 检查后续 2 字节是否也已到达；未到达则丢弃已读的首字节，
        // 宁可丢一包也不能阻塞（阻塞会让键鼠无响应）。
        if inb(STATUS_PORT) & 0x01 == 0 {
            return None;
        }
        let dx = inb(DATA_PORT) as i8;
        if inb(STATUS_PORT) & 0x01 == 0 {
            return None;
        }
        let dy = inb(DATA_PORT) as i8;
        Some((dx, dy, flags))
    }
}

/// 轮询鼠标并更新位置 / 按键。GUI 每次绘制前调用。
pub fn update() {
    if !ready() {
        return;
    }
    if let Some((dx, dy, flags)) = poll_packet() {
        unsafe {
            let (cols, rows) = vga_dims();
            X = (X + dx as i32).clamp(0, (cols as i32) * CELL_W - 1);
            // PS/2 的 Y 向上为正，屏幕坐标向下为正，因此取反
            Y = (Y - dy as i32).clamp(0, (rows as i32) * CELL_H - 1);

            let left = flags & 0x01 != 0;
            let right = flags & 0x02 != 0;
            if left && !LEFT {
                CLICK = Some((col(), row(), true));
            } else if right && !RIGHT {
                CLICK = Some((col(), row(), false));
            }
            LEFT = left;
            RIGHT = right;
        }
    }
}

#[inline]
fn vga_dims() -> (usize, usize) {
    // 使用实际探测到的控制台尺寸，兼容非 80x25 模式
    let (c, r) = crate::video::console();
    (c.max(40), r.max(20))
}

#[inline]
pub fn col() -> usize {
    unsafe { (X / CELL_W) as usize }
}

#[inline]
pub fn row() -> usize {
    unsafe { (Y / CELL_H) as usize }
}

/// 取出一次点击事件（列、行、是否左键），取后清空。
pub fn take_click() -> Option<(usize, usize, bool)> {
    unsafe { CLICK.take() }
}
