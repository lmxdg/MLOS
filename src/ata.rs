//! ATA/IDE PIO 硬盘驱动（Primary Master，LBA28）
//!
//! 支持 512 字节扇区的读写，用于系统配置的持久化存储。
//! 端口基址 0x1F0（主通道），设备 0xE0（主盘）。

use crate::io::{inb, inw, outb, outw};

const DATA: u16 = 0x1F0;
const SECTOR_COUNT: u16 = 0x1F2;
const LBA_LOW: u16 = 0x1F3;
const LBA_MID: u16 = 0x1F4;
const LBA_HIGH: u16 = 0x1F5;
const DRIVE_HEAD: u16 = 0x1F6;
const STATUS: u16 = 0x1F7;
const COMMAND: u16 = 0x1F7;

// 状态寄存器位
const STATUS_ERR: u8 = 0x01;
const STATUS_DRQ: u8 = 0x08;
const STATUS_SRV: u8 = 0x10;
const STATUS_DF: u8 = 0x20;
const STATUS_RDY: u8 = 0x40;
const STATUS_BSY: u8 = 0x80;

/// 检测主硬盘是否存在（DRDY 位）
pub fn detect() -> bool {
    unsafe {
        // 选择主盘，关闭中断
        outb(DRIVE_HEAD, 0xE0);
        // 等待 BSY 清除（最多 50ms 的轮询）
        for _ in 0..100_000 {
            if inb(STATUS) & STATUS_BSY == 0 {
                break;
            }
        }
        let status = inb(STATUS);
        // 无设备时总线浮空返回 0xFF
        if status == 0xFF || status & STATUS_ERR != 0 {
            return false;
        }
        status & STATUS_RDY != 0
    }
}

/// 等待 BSY 清除且 DRQ 置位，返回是否就绪
fn wait_for_drq() -> bool {
    unsafe {
        for _ in 0..5_000_000 {
            let status = inb(STATUS);
            if status & STATUS_ERR != 0 || status & STATUS_DF != 0 {
                return false;
            }
            if status & STATUS_BSY == 0 && status & STATUS_DRQ != 0 {
                return true;
            }
        }
        false
    }
}

/// 等待 BSY 清除（用于写命令后的完成等待）
fn wait_not_busy() -> bool {
    unsafe {
        for _ in 0..5_000_000 {
            let status = inb(STATUS);
            if status & STATUS_ERR != 0 || status & STATUS_DF != 0 {
                return false;
            }
            if status & STATUS_BSY == 0 {
                return true;
            }
        }
        false
    }
}

/// 向控制器发送 LBA28 参数
unsafe fn setup_lba(lba: u32, sector_count: u8) {
    outb(DRIVE_HEAD, 0xE0 | ((lba >> 24) & 0x0F) as u8);
    outb(SECTOR_COUNT, sector_count);
    outb(LBA_LOW, (lba & 0xFF) as u8);
    outb(LBA_MID, ((lba >> 8) & 0xFF) as u8);
    outb(LBA_HIGH, ((lba >> 16) & 0xFF) as u8);
}

/// 读取一个扇区（512 字节 = 256 words）到缓冲区
pub fn read_sector(lba: u32, buf: &mut [u16; 256]) -> bool {
    unsafe {
        if !wait_not_busy() {
            return false;
        }
        setup_lba(lba, 1);
        outb(COMMAND, 0x20); // READ SECTORS (PIO, 28-bit LBA)
        if !wait_for_drq() {
            return false;
        }
        for word in buf.iter_mut() {
            *word = inw(DATA);
        }
        true
    }
}

/// 写入一个扇区（256 words）并等待完成
pub fn write_sector(lba: u32, buf: &[u16; 256]) -> bool {
    unsafe {
        if !wait_not_busy() {
            return false;
        }
        setup_lba(lba, 1);
        outb(COMMAND, 0x30); // WRITE SECTORS (PIO, 28-bit LBA)
        if !wait_for_drq() {
            return false;
        }
        for &word in buf.iter() {
            outw(DATA, word);
        }
        // 等待写入完成
        if !wait_not_busy() {
            return false;
        }
        // CACHE FLUSH，确保数据真正落盘
        outb(COMMAND, 0xE7);
        wait_not_busy()
    }
}
