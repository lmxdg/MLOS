//! 底层 x86 I/O 端口操作

#[inline(always)]
pub unsafe fn outb(port: u16, value: u8) {
    core::arch::asm!(
        "out dx, al",
        in("dx") port,
        in("al") value,
        options(nomem, nostack, preserves_flags)
    );
}

#[inline(always)]
pub unsafe fn inb(port: u16) -> u8 {
    let value: u8;
    core::arch::asm!(
        "in al, dx",
        out("al") value,
        in("dx") port,
        options(nomem, nostack, preserves_flags)
    );
    value
}

#[inline(always)]
pub unsafe fn outw(port: u16, value: u16) {
    core::arch::asm!(
        "out dx, ax",
        in("dx") port,
        in("ax") value,
        options(nomem, nostack, preserves_flags)
    );
}

#[inline(always)]
pub unsafe fn inw(port: u16) -> u16 {
    let value: u16;
    core::arch::asm!(
        "in ax, dx",
        out("ax") value,
        in("dx") port,
        options(nomem, nostack, preserves_flags)
    );
    value
}

/// 读时间戳计数器，用作随机数种子
#[inline(always)]
pub unsafe fn rdtsc() -> u64 {
    let lo: u32;
    let hi: u32;
    core::arch::asm!(
        "rdtsc",
        out("eax") lo,
        out("edx") hi,
        options(nomem, nostack)
    );
    ((hi as u64) << 32) | lo as u64
}

/// 初始化串口 COM1 (115200 8N1)，用于调试输出
#[inline(always)]
pub unsafe fn serial_init() {
    outb(0x3F8, 0x00); // 关闭中断
    outb(0x3F9, 0x00);
    outb(0x3FB, 0x80); // 启用 DLAB 设置波特率
    outb(0x3F8, 0x01); // 除数低字节 (115200/1=115200)
    outb(0x3F9, 0x00); // 除数高字节
    outb(0x3FB, 0x03); // 8 位数据, 1 位停止, 无校验, 关闭 DLAB
}

pub unsafe fn serial_putchar(c: u8) {
    // 同时写到 COM1(0x3F8) 和 QEMU isa-debugcon(0x402)
    let mut timeout = 100_000u32;
    while inb(0x3FD) & 0x20 == 0 && timeout > 0 {
        timeout -= 1;
    }
    outb(0x3F8, c);
    outb(0x402, c);
}

pub unsafe fn serial_putstr(s: &str) {
    for b in s.bytes() {
        serial_putchar(b);
    }
}

/// 纯 QEMU debugcon 输出（端口 0x402），无串口 FIFO 干扰
pub unsafe fn dbg(s: &str) {
    for b in s.bytes() {
        outb(0x402, b);
    }
}
