//! CMOS 实时时钟（RTC）。

use crate::io::{inb, outb};

#[derive(Clone, Copy)]
pub struct DateTime {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    pub weekday: u8,
}

fn cmos(reg: u8) -> u8 {
    unsafe {
        outb(0x70, reg);
        inb(0x71)
    }
}

fn bcd(v: u8) -> u8 {
    (v & 0x0F) + ((v >> 4) * 10)
}

fn wait_not_updating() {
    for _ in 0..1_000_000 {
        if cmos(0x0A) & 0x80 == 0 {
            return;
        }
        unsafe { core::arch::asm!("pause"); }
    }
}

pub fn now() -> DateTime {
    wait_not_updating();
    let sec = cmos(0x00);
    let min = cmos(0x02);
    let hour = cmos(0x04);
    let weekday = cmos(0x06);
    let day = cmos(0x07);
    let month = cmos(0x08);
    let year = cmos(0x09);
    let statb = cmos(0x0B);
    let bcd_mode = statb & 0x04 == 0;
    let hour12 = statb & 0x02 == 0;
    let pm = hour & 0x80 != 0;

    let (mut hour, min, sec, day, month, year) = if bcd_mode {
        (bcd(hour & 0x7F), bcd(min), bcd(sec), bcd(day), bcd(month), bcd(year))
    } else {
        (hour & 0x7F, min, sec, day, month, year)
    };
    if hour12 {
        hour = hour % 12;
        if pm {
            hour += 12;
        }
    }
    DateTime {
        year: 2000 + year as u16,
        month,
        day,
        hour,
        minute: min,
        second: sec,
        weekday: if bcd_mode { bcd(weekday) } else { weekday },
    }
}

pub fn weekday_name(w: u8) -> &'static str {
    match w & 0x07 {
        0 => "Sun",
        1 => "Mon",
        2 => "Tue",
        3 => "Wed",
        4 => "Thu",
        5 => "Fri",
        _ => "Sat",
    }
}

pub fn month_name(m: u8) -> &'static str {
    match m {
        1 => "Jan", 2 => "Feb", 3 => "Mar", 4 => "Apr",
        5 => "May", 6 => "Jun", 7 => "Jul", 8 => "Aug",
        9 => "Sep", 10 => "Oct", 11 => "Nov", 12 => "Dec",
        _ => "???",
    }
}