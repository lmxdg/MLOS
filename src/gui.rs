//! MLOS 图形桌面（VBE framebuffer + PS/2 鼠标）。
//!
//! Linux 风格：顶部面板、左侧图标、可拖动窗口、底部任务栏、应用菜单。
//! 所有文本使用 8x16 ASCII / 16x16 中文点阵直接绘制到帧缓冲，不再依赖
//! VGA 文本模式或字模重编程。绘制过程不使用堆分配（bump allocator 不会
//! 回收），所有格式化都写入栈上的小缓冲区。

use crate::fb;
use crate::fs;
use crate::hw;
use crate::installer;
use crate::keyboard::{self, Key, Keyboard};
use crate::mouse;
use crate::perf;
use crate::rtc;
use crate::usb;
use crate::vga;
use core::fmt::{self, Write};

const PANEL_H: i32 = 28;
const TASKBAR_H: i32 = 30;

const BG: u32 = 0x1C202A;
const WALL: u32 = 0x171B24;
const PANEL: u32 = 0x262B36;
const PANEL_DARK: u32 = 0x1E222B;
const TEXT: u32 = 0xE6E9EF;
const MUTED: u32 = 0x9AA3B2;
const ACCENT: u32 = 0x409EFF;
const GREEN: u32 = 0x4CD964;
const RED: u32 = 0xFF5F57;
const YELLOW: u32 = 0xFFCC00;
const WIN_BG: u32 = 0xF2F4F8;
const WIN_TEXT: u32 = 0x1C202A;
const WIN_MUTED: u32 = 0x5A6472;
const ROW_SEL: u32 = 0xCBE3FF;
const SHADOW: u32 = 0x0B0E13;

#[derive(Clone, Copy, PartialEq, Eq)]
enum App { Perf, Files, View, Usb, About }

#[derive(Clone, Copy)]
struct Win {
    app: App,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    scroll: usize,
}

#[derive(Clone, Copy)]
struct Rect { x: i32, y: i32, w: i32, h: i32 }

impl Rect {
    fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }
}

struct Gui {
    wins: [Option<Win>; 4],
    order: [usize; 4],
    nwin: usize,
    focus: usize,
    menu_open: bool,
    menu_sel: usize,
    files_sel: usize,
    view_name: [u8; 32],
    view_len: usize,
    drag: Option<(usize, i32, i32)>,
}

const MENU_LEN: usize = 7;

fn menu_label(i: usize) -> &'static str {
    let zh = vga::lang_zh();
    match i {
        0 => if zh { "性能监视器" } else { "Performance Monitor" },
        1 => if zh { "文件管理器" } else { "File Manager" },
        2 => if zh { "USB 设备" } else { "USB Devices" },
        3 => if zh { "关于 MLOS" } else { "About MLOS" },
        4 => if zh { "终端（返回 Shell）" } else { "Terminal (back to Shell)" },
        5 => if zh { "切换语言 EN / 中文" } else { "Switch language EN / 中文" },
        _ => if zh { "重新启动" } else { "Reboot" },
    }
}

fn icon_label(i: usize) -> &'static str {
    let zh = vga::lang_zh();
    match i {
        0 => if zh { "性能" } else { "Perf" },
        1 => if zh { "文件" } else { "Files" },
        2 => if zh { "USB" } else { "USB" },
        3 => if zh { "关于" } else { "About" },
        _ => if zh { "终端" } else { "Terminal" },
    }
}

fn app_title(app: App) -> &'static str {
    let zh = vga::lang_zh();
    match app {
        App::Perf => if zh { "性能监视器" } else { "Performance Monitor" },
        App::Files => if zh { "文件管理器" } else { "File Manager" },
        App::View => if zh { "文件查看器" } else { "File Viewer" },
        App::Usb => if zh { "USB 设备" } else { "USB Devices" },
        App::About => if zh { "关于 MLOS" } else { "About MLOS" },
    }
}

struct Buf<const N: usize> { d: [u8; N], n: usize }

impl<const N: usize> Buf<N> {
    fn new() -> Self { Self { d: [0; N], n: 0 } }
    fn s(&self) -> &str { core::str::from_utf8(&self.d[..self.n]).unwrap_or("") }
}

impl<const N: usize> fmt::Write for Buf<N> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for b in s.bytes() {
            if self.n < N { self.d[self.n] = b; self.n += 1; }
        }
        Ok(())
    }
}

fn text(x: i32, y: i32, s: &str, fg: u32, bg: u32) {
    fb::draw_text(x, y, s, fg, bg);
}

fn text_center(y: i32, s: &str, fg: u32, bg: u32) {
    let w = crate::font::text_width(s) as i32;
    text((fb::width() as i32 - w) / 2, y, s, fg, bg);
}

fn icon_rect(i: usize) -> Rect {
    Rect { x: 18, y: 52 + i as i32 * 82, w: 110, h: 66 }
}

fn menu_rect() -> Rect {
    Rect { x: 66, y: 30, w: 286, h: MENU_LEN as i32 * 30 + 10 }
}

fn start_rect() -> Rect { Rect { x: 8, y: fb::height() as i32 - TASKBAR_H + 4, w: 84, h: 22 } }
fn apps_rect() -> Rect { Rect { x: 120, y: 3, w: 86, h: 22 } }
fn lang_rect() -> Rect { Rect { x: 212, y: 3, w: 98, h: 22 } }

fn draw_icon(i: usize, x: i32, y: i32, hover: bool, bg: u32) {
    let tile = if hover { 0x2E3B52 } else { 0x222A38 };
    fb::fill_rect(x, y, 56, 56, tile);
    fb::fill_rect(x + 1, y + 1, 54, 54, if hover { 0x34435C } else { 0x252E3D });
    let fg = if hover { TEXT } else { 0xC9D2E2 };
    match i {
        0 => {
            fb::fill_rect(x + 12, y + 12, 6, 28, GREEN);
            fb::fill_rect(x + 24, y + 20, 6, 20, ACCENT);
            fb::fill_rect(x + 36, y + 28, 6, 12, YELLOW);
            fb::draw_hline(x + 10, y + 42, 36, fg);
        }
        1 => {
            fb::fill_rect(x + 10, y + 16, 36, 26, YELLOW);
            fb::fill_rect(x + 10, y + 12, 16, 8, YELLOW);
            fb::fill_rect(x + 13, y + 19, 30, 20, 0x2B3242);
            fb::draw_hline(x + 10, y + 42, 36, YELLOW);
        }
        2 => {
            fb::draw_line(x + 14, y + 42, x + 14, y + 18, fg);
            fb::draw_line(x + 36, y + 42, x + 36, y + 18, fg);
            fb::draw_line(x + 25, y + 18, x + 25, y + 42, fg);
            fb::draw_line(x + 25, y + 28, x + 14, y + 36, fg);
            fb::draw_line(x + 25, y + 28, x + 36, y + 36, fg);
            fb::fill_rect(x + 11, y + 14, 6, 6, fg);
            fb::fill_rect(x + 33, y + 14, 6, 6, fg);
            fb::fill_rect(x + 22, y + 39, 6, 6, fg);
        }
        3 => {
            fb::fill_rect(x + 28, y + 11, 3, 24, fg);
            fb::fill_rect(x + 24, y + 9, 11, 3, fg);
            fb::fill_rect(x + 19, y + 35, 20, 3, fg);
            text(x + 22, y + 14, "i", bg, fg);
        }
        _ => {
            fb::fill_rect(x + 9, y + 12, 38, 30, 0x10141C);
            text(x + 12, y + 16, ">_", fg, 0x10141C);
        }
    }
    let label = icon_label(i);
    let w = crate::font::text_width(label) as i32;
    text(x + (56 - w) / 2, y + 60, label, TEXT, bg);
}

impl Gui {
    fn new() -> Self {
        let mut g = Self {
            wins: [None; 4],
            order: [0; 4],
            nwin: 0,
            focus: 0,
            menu_open: false,
            menu_sel: 0,
            files_sel: 0,
            view_name: [0; 32],
            view_len: 0,
            drag: None,
        };
        g.open(App::About);
        g
    }

    fn focused_slot(&self) -> Option<usize> {
        if self.nwin == 0 { None } else { Some(self.order[self.nwin - 1]) }
    }

    fn raise(&mut self, slot: usize) {
        if self.nwin == 0 { return; }
        if self.order[self.nwin - 1] == slot { self.focus = slot; return; }
        let mut pos = None;
        for i in 0..self.nwin { if self.order[i] == slot { pos = Some(i); break; } }
        if let Some(p) = pos {
            for i in p..self.nwin - 1 { self.order[i] = self.order[i + 1]; }
            self.order[self.nwin - 1] = slot;
        }
        self.focus = slot;
    }

    fn remove(&mut self, slot: usize) {
        let mut pos = None;
        for i in 0..self.nwin { if self.order[i] == slot { pos = Some(i); break; } }
        if let Some(p) = pos {
            for i in p..self.nwin - 1 { self.order[i] = self.order[i + 1]; }
            self.nwin -= 1;
        }
        self.wins[slot] = None;
        if self.nwin > 0 { self.focus = self.order[self.nwin - 1]; } else { self.focus = 0; }
    }

    fn open(&mut self, app: App) {
        let slot = if let Some(s) = self.wins.iter().position(|w| w.is_none()) {
            s
        } else if self.nwin > 0 {
            let s = self.order[0];
            self.remove(s);
            s
        } else { 0 };

        let sw = fb::width() as i32;
        let sh = fb::height() as i32;
        let w = (sw - 120).min(520).max(360);
        let h = (sh - 140).min(380).max(260);
        let idx = self.nwin as i32;
        let mut x = 190 + idx * 34;
        let mut y = 70 + idx * 34;
        if x + w > sw - 20 { x = sw - w - 20; }
        if y + h > sh - TASKBAR_H - 10 { y = sh - TASKBAR_H - h - 10; }
        if x < 140 { x = 140; }
        if y < PANEL_H + 10 { y = PANEL_H + 10; }

        self.wins[slot] = Some(Win {
            app,
            x, y, w, h,
            scroll: 0,
        });
        self.order[self.nwin] = slot;
        self.nwin += 1;
        self.raise(slot);
    }

    fn set_view_name(&mut self, name: &str) {
        self.view_name = [0; 32];
        let n = name.len().min(31);
        self.view_name[..n].copy_from_slice(&name.as_bytes()[..n]);
        self.view_len = n;
    }

    fn view_name_str(&self) -> &str {
        core::str::from_utf8(&self.view_name[..self.view_len]).unwrap_or("")
    }

    fn count_files(&self) -> usize {
        fs::list().map(|(_, c)| c as usize).unwrap_or(0)
    }

    fn nth_file(&self, i: usize) -> Option<&'static str> {
        static mut NAMES: [[u8; 30]; 12] = [[0; 30]; 12];
        static mut COUNT: usize = 0;
        unsafe {
            let (entries, count) = fs::list()?;
            COUNT = count;
            for (idx, e) in entries.iter().flatten().enumerate() { NAMES[idx] = e.name; }
            if i < COUNT {
                let len = NAMES[i].iter().position(|&b| b == 0).unwrap_or(30);
                Some(core::str::from_utf8(&NAMES[i][..len]).unwrap_or(""))
            } else { None }
        }
    }
}

fn draw_panel(g: &Gui, mx: i32, my: i32) {
    let w = fb::width() as i32;
    fb::fill_rect(0, 0, w, PANEL_H, PANEL);
    fb::draw_hline(0, PANEL_H - 1, w, 0x141821);
    text(10, 6, "MLOS Desktop", GREEN, PANEL);

    let ab = apps_rect();
    let abg = if ab.contains(mx, my) || g.menu_open { 0x34435C } else { 0x2B3342 };
    fb::fill_rect(ab.x, ab.y, ab.w, ab.h, abg);
    let alabel = crate::tr!("[Apps]", "[应用]");
    text(ab.x + (ab.w - crate::font::text_width(alabel) as i32) / 2, ab.y + 3, alabel, TEXT, abg);

    let lb = lang_rect();
    let lbg = if lb.contains(mx, my) { 0x34435C } else { 0x2B3342 };
    fb::fill_rect(lb.x, lb.y, lb.w, lb.h, lbg);
    text(lb.x + 8, lb.y + 3, "[EN/中]", TEXT, lbg);

    let title = crate::tr!("MLOS Desktop", "MLOS 桌面");
    let tw = crate::font::text_width(title) as i32;
    text((w - tw) / 2, 6, title, MUTED, PANEL);

    let t = rtc::now();
    let mut b = Buf::<16>::new();
    let time = if t.hour <= 23 && t.minute <= 59 && t.second <= 59 {
        let _ = write!(b, "{:02}:{:02}:{:02}", t.hour, t.minute, t.second);
        b.s()
    } else { "--:--:--" };
    let cw = crate::font::text_width(time) as i32;
    text(w - cw - 12, 6, time, TEXT, PANEL);
}

fn draw_desktop(g: &Gui, mx: i32, my: i32) {
    let w = fb::width() as i32;
    let h = fb::height() as i32;
    fb::clear(BG);
    // 壁纸：淡色 MLOS 标记与细网格
    for y in (PANEL_H + 24..h - TASKBAR_H).step_by(64) {
        fb::draw_hline(0, y, w, 0x202632);
    }
    let mark = "MLOS  Minimal Light Operating System";
    let mw = crate::font::text_width(mark) as i32;
    text((w - mw) / 2, h / 2 - 60, mark, 0x2C3444, BG);

    for i in 0..5 {
        let r = icon_rect(i);
        draw_icon(i, r.x + 6, r.y, r.contains(mx, my), BG);
    }
    let _ = g;
}

fn draw_menu(g: &Gui, mx: i32, my: i32) {
    if !g.menu_open { return; }
    let r = menu_rect();
    fb::fill_rect(r.x + 6, r.y + 6, r.w, r.h, SHADOW);
    fb::fill_rect(r.x, r.y, r.w, r.h, PANEL);
    fb::fill_rect(r.x + 1, r.y + 1, r.w - 2, r.h - 2, 0x2B3342);
    for i in 0..MENU_LEN {
        let y = r.y + 5 + i as i32 * 30;
        let item = Rect { x: r.x + 5, y, w: r.w - 10, h: 28 };
        let sel = item.contains(mx, my) || i == g.menu_sel;
        let bg = if sel { ACCENT } else { 0x2B3342 };
        fb::fill_rect(item.x, item.y, item.w, item.h, bg);
        text(item.x + 10, item.y + 5, menu_label(i), if sel { 0xFFFFFF } else { TEXT }, bg);
        if i == 4 || i == 6 {
            fb::draw_hline(item.x + 8, item.y + 27, item.w - 16, if sel { 0xFFFFFF } else { 0x3A4A63 });
        }
    }
}

fn draw_window(g: &Gui, win: &Win, focused: bool, mx: i32, my: i32) {
    let x = win.x;
    let y = win.y;
    fb::fill_rect(x + 6, y + 6, win.w, win.h, SHADOW);
    fb::fill_rect(x, y, win.w, win.h, if focused { ACCENT } else { 0x55606F });
    let tbg = if focused { 0x34435C } else { 0x2A303B };
    fb::fill_rect(x + 2, y + 2, win.w - 4, 24, tbg);
    text(x + 10, y + 5, app_title(win.app), TEXT, tbg);

    let close = Rect { x: x + win.w - 24, y: y + 3, w: 19, h: 19 };
    let cbg = if close.contains(mx, my) { RED } else { if focused { 0x44546F } else { 0x3A414E } };
    fb::fill_rect(close.x, close.y, close.w, close.h, cbg);
    text(close.x + 6, close.y + 2, "x", 0xFFFFFF, cbg);

    fb::fill_rect(x + 2, y + 26, win.w - 4, win.h - 28, WIN_BG);
    draw_app(g, win, x + 10, y + 34, win.w - 20, win.h - 44);
}

fn draw_app(g: &Gui, win: &Win, x: i32, y: i32, w: i32, h: i32) {
    let bottom = y + h;
    match win.app {
        App::Perf => draw_perf(x, y, w, bottom),
        App::Files => draw_files(g, win, x, y, w, bottom),
        App::View => draw_view(g, win, x, y, w, bottom),
        App::Usb => draw_usb(x, y, bottom),
        App::About => draw_about(x, y, bottom),
    }
}

fn draw_perf(x: i32, y: i32, w: i32, bottom: i32) {
    let cpu = hw::cpu();
    let vendor = hw::vendor_str(&cpu);
    let info = perf::cpu_info();
    let mem = perf::mem_info();
    let disk = perf::disk_info();
    let bh = Buf::<64>::new();
    let mut yy = y;
    let mut b = bh;
    if vga::lang_zh() { let _ = write!(b, "处理器  : {}", vendor); }
    else { let _ = write!(b, "CPU     : {}", vendor); }
    text(x, yy, b.s(), WIN_TEXT, WIN_BG); yy += 20;
    b = Buf::new();
    if vga::lang_zh() { let _ = write!(b, "TSC     : ~{} MHz", info.tsc_hz / 1_000_000); }
    else { let _ = write!(b, "TSC     : ~{} MHz", info.tsc_hz / 1_000_000); }
    text(x, yy, b.s(), WIN_TEXT, WIN_BG); yy += 20;
    b = Buf::new();
    if vga::lang_zh() {
        let _ = write!(b, "内存    : {} MiB / 内核 {} KiB", mem.total_kib / 1024, mem.kernel_kib);
    } else {
        let _ = write!(b, "Memory  : {} MiB total / {} KiB kernel", mem.total_kib / 1024, mem.kernel_kib);
    }
    text(x, yy, b.s(), WIN_TEXT, WIN_BG); yy += 20;
    b = Buf::new();
    let use_pct = if mem.total_kib > 0 {
        ((mem.kernel_kib * 100) / mem.total_kib).min(100) as i32
    } else { 0 };
    if vga::lang_zh() { let _ = write!(b, "使用率  : {}%", use_pct); }
    else { let _ = write!(b, "Mem use : {}%", use_pct); }
    text(x, yy, b.s(), WIN_TEXT, WIN_BG); yy += 18;
    // 内存条
    let bar_w = w - 120;
    fb::fill_rect(x, yy, bar_w, 12, 0xD8DEE8);
    fb::fill_rect(x, yy, bar_w * use_pct / 100, 12, ACCENT);
    yy += 24;
    b = Buf::new();
    if vga::lang_zh() {
        let _ = write!(b, "硬盘    : {} MiB / 已用 {} 扇区", disk.total_sectors / 2048, disk.used_sectors);
    } else {
        let _ = write!(b, "Disk    : {} MiB / used {} sectors", disk.total_sectors / 2048, disk.used_sectors);
    }
    text(x, yy, b.s(), WIN_TEXT, WIN_BG); yy += 20;
    b = Buf::new();
    let pct = if disk.total_sectors > 0 { (disk.used_sectors * 100 / disk.total_sectors).min(100) as i32 } else { 0 };
    if vga::lang_zh() { let _ = write!(b, "硬盘使用: {}%", pct); }
    else { let _ = write!(b, "Disk use: {}%", pct); }
    text(x, yy, b.s(), WIN_TEXT, WIN_BG); yy += 18;
    let bar_w = w - 120;
    fb::fill_rect(x, yy, bar_w, 12, 0xD8DEE8);
    fb::fill_rect(x, yy, bar_w * pct / 100, 12, GREEN);
    yy += 24;
    b = Buf::new();
    let boot = if vga::lang_zh() {
        if installer::is_installed() { "硬盘引导已安装" } else { "仅 ISO（可运行 bootinstall）" }
    } else if installer::is_installed() { "disk boot installed" } else { "ISO only (run bootinstall)" };
    if vga::lang_zh() { let _ = write!(b, "启动    : {}", boot); }
    else { let _ = write!(b, "Boot    : {}", boot); }
    text(x, yy, b.s(), WIN_MUTED, WIN_BG);
    let _ = bottom;
}

fn draw_files(g: &Gui, win: &Win, x: i32, y: i32, w: i32, bottom: i32) {
    let user = crate::shell::current_user();
    let mut b = Buf::<48>::new();
    let _ = write!(b, "~/home/{}", user);
    text(x, y, b.s(), WIN_MUTED, WIN_BG);
    fb::draw_hline(x, y + 18, w, 0xC9D1DC);
    text(x, y + 24, if vga::lang_zh() { "名称" } else { "NAME" }, WIN_MUTED, WIN_BG);
    text(x + w - 90, y + 24, if vga::lang_zh() { "大小" } else { "SIZE" }, WIN_MUTED, WIN_BG);
    fb::draw_hline(x, y + 42, w, 0xC9D1DC);

    let (entries, count) = match fs::list() {
        Some(v) => v,
        None => ([None; 12], 0),
    };
    let row_h = 20;
    if count == 0 {
        text(x, y + 50, if vga::lang_zh() { "（空，使用 write 命令创建文件）" } else { "(empty - use 'write <name> <text>')" }, WIN_MUTED, WIN_BG);
    }
    let mut idx = 0usize;
    for e in entries.iter().flatten() {
        let ry = y + 48 + (idx.saturating_sub(win.scroll) as i32) * row_h;
        if idx >= win.scroll && ry + row_h <= bottom - 22 {
            let selected = idx == g.files_sel;
            let bg = if selected { ROW_SEL } else { WIN_BG };
            if selected { fb::fill_rect(x - 4, ry - 2, w + 8, row_h, ROW_SEL); }
            text(x, ry, e.name_str(), if selected { WIN_TEXT } else { WIN_TEXT }, bg);
            let mut sb = Buf::<24>::new();
            let _ = write!(sb, "{} B", e.size);
            text(x + w - 90, ry, sb.s(), WIN_MUTED, bg);
        }
        idx += 1;
    }
    let hint = if vga::lang_zh() {
        "点击 / Enter 打开 · D 删除 · 上下 选择"
    } else {
        "Click / Enter open · D delete · Up/Down select"
    };
    text(x, bottom - 18, hint, WIN_MUTED, WIN_BG);
}

fn view_text(g: &Gui) -> ([u8; 4096], usize) {
    let mut buf = [0u8; 4096];
    let n = fs::read(g.view_name_str(), &mut buf).unwrap_or(0);
    (buf, n)
}

fn count_wrapped(text: &str, maxw: i32) -> usize {
    let mut lines = 1usize;
    let mut cx = 0i32;
    for ch in text.chars() {
        if ch == '\n' { lines += 1; cx = 0; continue; }
        let cw = crate::font::char_width(ch as u32) as i32;
        if cx + cw > maxw { lines += 1; cx = 0; }
        cx += cw;
    }
    lines
}

fn draw_view(g: &Gui, win: &Win, x: i32, y: i32, w: i32, bottom: i32) {
    let (buf, n) = view_text(g);
    let txt = core::str::from_utf8(&buf[..n]).unwrap_or("(binary data)");
    let mut b = Buf::<48>::new();
    let _ = write!(b, "{} ({} bytes)", g.view_name_str(), n);
    text(x, y, b.s(), WIN_MUTED, WIN_BG);
    fb::draw_hline(x, y + 18, w, 0xC9D1DC);

    let maxw = w - 18;
    let total = count_wrapped(txt, maxw);
    let visible = ((bottom - y - 60) / 16).max(1) as usize;
    let scroll = win.scroll.min(total.saturating_sub(1));
    let mut line = 0usize;
    let mut cx = x;
    let mut cy = y + 26;
    for ch in txt.chars() {
        if ch == '\n' {
            line += 1; cx = x; cy += 16;
            if cy + 16 > bottom - 20 { break; }
            continue;
        }
        if ch == '\r' { continue; }
        let cw = crate::font::char_width(ch as u32) as i32;
        if cx + cw > x + maxw {
            line += 1; cx = x; cy += 16;
            if cy + 16 > bottom - 20 { break; }
        }
        if line >= scroll {
            let mut tmp = [0u8; 4];
            let s = ch.encode_utf8(&mut tmp);
            text(cx, cy, s, WIN_TEXT, WIN_BG);
        }
        cx += cw;
    }
    let mut hint = Buf::<80>::new();
    if vga::lang_zh() {
        let _ = write!(hint, "行 {}/{} · 上下滚动 · Esc 关闭", scroll + 1, total);
    } else {
        let _ = write!(hint, "line {}/{} · Up/Down scroll · Esc close", scroll + 1, total);
    }
    text(x, bottom - 18, hint.s(), WIN_MUTED, WIN_BG);
    let _ = visible;
}

fn draw_usb(x: i32, y: i32, bottom: i32) {
    let n = usb::count();
    let mut b = Buf::<64>::new();
    if !usb::is_scanned() {
        let _ = write!(b, "{}", if vga::lang_zh() { "总线尚未扫描。" } else { "Bus not scanned yet." });
    } else if n == 0 {
        let _ = write!(b, "{}", if vga::lang_zh() { "未检测到 USB 主控。" } else { "No USB host controller detected." });
    } else {
        if vga::lang_zh() { let _ = write!(b, "{} 个 USB 主控 - USB 1.0..3.2", n); }
        else { let _ = write!(b, "{} USB host controller(s) - USB 1.0..3.2", n); }
    }
    text(x, y, b.s(), WIN_TEXT, WIN_BG);
    let mut yy = y + 26;
    let mut idx = 0usize;
    usb::each(|c| {
        idx += 1;
        if yy + 16 < bottom - 20 {
            let mut line = Buf::<96>::new();
            let _ = write!(line, "[{}] {:02X}:{:02X}.{} {}", c.kind, c.bus, c.device, c.function, c.speed.label());
            text(x, yy, line.s(), WIN_TEXT, WIN_BG);
            yy += 22;
        }
    });
    if idx == 0 && usb::is_scanned() {
        text(x, yy, if vga::lang_zh() { "可在 Shell 运行 usb 命令完整扫描。" } else { "Run 'usb' in the shell for a full scan." }, WIN_MUTED, WIN_BG);
    }
}

fn draw_about(x: i32, y: i32, bottom: i32) {
    let zh = vga::lang_zh();
    let lines: [&str; 9] = [
        "MLOS - Minimal Light Operating System",
        if zh { "版本 1.0.6  x86_64  Rust no_std" } else { "Version 1.0.6  x86_64  Rust no_std" },
        if zh { "开发者: MEME-LMXDG" } else { "Developer: MEME-LMXDG" },
        "",
        if zh { "帧缓冲控制台 + 中文点阵字体" } else { "Framebuffer console + bitmap CJK font" },
        if zh { "鼠标驱动的窗口管理器" } else { "Mouse-driven window manager" },
        if zh { "ATA PIO 存储 / MiniFS 文件系统" } else { "ATA PIO storage / MiniFS" },
        if zh { "安装向导内置硬件检测" } else { "Hardware check in setup wizard" },
        if zh { "Esc: 关闭窗口 / 退出桌面" } else { "Esc: close window / quit desktop" },
    ];
    let mut yy = y;
    for (i, s) in lines.iter().enumerate() {
        if yy + 16 > bottom - 10 { break; }
        let fg = if i == 0 { ACCENT } else if i == 2 { WIN_MUTED } else { WIN_TEXT };
        text(x, yy, s, fg, WIN_BG);
        yy += 20;
    }
}

fn draw_taskbar(g: &Gui, mx: i32, my: i32) {
    let w = fb::width() as i32;
    let h = fb::height() as i32;
    let top = h - TASKBAR_H;
    fb::fill_rect(0, top, w, TASKBAR_H, PANEL_DARK);
    fb::draw_hline(0, top, w, 0x10141C);

    let sr = start_rect();
    let sbg = if sr.contains(mx, my) || g.menu_open { 0x34435C } else { 0x252C39 };
    fb::fill_rect(sr.x, sr.y, sr.w, sr.h, sbg);
    let slabel = crate::tr!("[Start]", "[开始]");
    text(sr.x + (sr.w - crate::font::text_width(slabel) as i32) / 2, sr.y + 3, slabel, TEXT, sbg);

    let mut bx = 100;
    for i in 0..g.nwin {
        let slot = g.order[i];
        if let Some(win) = g.wins[slot] {
            let r = Rect { x: bx, y: top + 4, w: 132, h: 22 };
            let active = slot == g.focused_slot().unwrap_or(usize::MAX);
            let bg = if r.contains(mx, my) { 0x34435C } else if active { 0x2E3A4E } else { 0x252C39 };
            fb::fill_rect(r.x, r.y, r.w, r.h, bg);
            let title = app_title(win.app);
            text(r.x + 8, r.y + 3, title, if active { TEXT } else { MUTED }, bg);
            bx += 136;
        }
    }
    let hint = crate::tr!("Esc close · M menu · Tab focus", "Esc 关闭 · M 菜单 · Tab 切换");
    let hw = crate::font::text_width(hint) as i32;
    if bx + hw + 20 < w {
        text(w - hw - 12, top + 7, hint, MUTED, PANEL_DARK);
    }
}

fn draw_all(g: &Gui) {
    let mx = mouse::x();
    let my = mouse::y();
    draw_desktop(g, mx, my);
    for i in 0..g.nwin {
        let slot = g.order[i];
        if let Some(win) = g.wins[slot] {
            draw_window(g, &win, slot == g.focused_slot().unwrap_or(usize::MAX), mx, my);
        }
    }
    draw_panel(g, mx, my);
    draw_taskbar(g, mx, my);
    draw_menu(g, mx, my);
}

fn activate(g: &mut Gui, app: App) -> bool {
    match app {
        App::Files => g.open(App::Files),
        App::Perf => g.open(App::Perf),
        App::Usb => g.open(App::Usb),
        App::About => g.open(App::About),
        App::View => {}
    }
    false
}

fn activate_icon(g: &mut Gui, i: usize) -> bool {
    match i {
        0 => activate(g, App::Perf),
        1 => activate(g, App::Files),
        2 => activate(g, App::Usb),
        3 => activate(g, App::About),
        _ => true, // Terminal: 返回 Shell
    }
}

fn activate_menu(g: &mut Gui, i: usize) -> bool {
    match i {
        0 => activate(g, App::Perf),
        1 => activate(g, App::Files),
        2 => activate(g, App::Usb),
        3 => activate(g, App::About),
        4 => return true,
        5 => {
            vga::set_lang(!vga::lang_zh());
            false
        }
        _ => {
            crate::println!("Rebooting...");
            unsafe { crate::io::outb(0x64, 0xFE); }
            crate::halt_loop();
        }
    }
}

fn open_view(g: &mut Gui, idx: usize) {
    if let Some(name) = g.nth_file(idx) {
        g.files_sel = idx;
        g.set_view_name(name);
        g.open(App::View);
    }
}

fn handle_click(g: &mut Gui, cx: i32, cy: i32) -> bool {
    let mx = cx;
    let my = cy;

    if g.menu_open {
        let r = menu_rect();
        if r.contains(mx, my) {
            let i = ((my - r.y - 5) / 30) as usize;
            if i < MENU_LEN {
                g.menu_open = false;
                return activate_menu(g, i);
            }
        } else {
            g.menu_open = false;
        }
    }

    if apps_rect().contains(mx, my) || start_rect().contains(mx, my) {
        g.menu_open = !g.menu_open;
        g.menu_sel = 0;
        return false;
    }
    if lang_rect().contains(mx, my) {
        vga::set_lang(!vga::lang_zh());
        return false;
    }

    // 任务栏窗口按钮
    if cy >= fb::height() as i32 - TASKBAR_H {
        let mut bx = 100;
        for i in 0..g.nwin {
            let slot = g.order[i];
            if g.wins[slot].is_some() {
                if mx >= bx && mx < bx + 132 {
                    g.raise(slot);
                    return false;
                }
                bx += 136;
            }
        }
        return false;
    }

    // 桌面图标
    for i in 0..5 {
        if icon_rect(i).contains(mx, my) {
            return activate_icon(g, i);
        }
    }

    // 窗口（从最上层向下）
    let mut idx = g.nwin;
    while idx > 0 {
        idx -= 1;
        let slot = g.order[idx];
        let Some(win) = g.wins[slot] else { continue; };
        if mx >= win.x && mx < win.x + win.w && my >= win.y && my < win.y + win.h {
            g.raise(slot);
            if my < win.y + 26 {
                if mx >= win.x + win.w - 24 {
                    g.remove(slot);
                } else {
                    g.drag = Some((slot, mx - win.x, my - win.y));
                }
            } else {
                match win.app {
                    App::Files => {
                        let row_h = 20;
                        let first = win.y + 34 + 48;
                        let rel = my - first;
                        if rel >= 0 {
                            let sel = rel / row_h;
                            let fi = win.scroll + sel as usize;
                            if fi < g.count_files() {
                                open_view(g, fi);
                            }
                        }
                    }
                    App::View => {
                        let mut w2 = win;
                        if my < win.y + 60 {
                            w2.scroll = w2.scroll.saturating_sub(3);
                        } else {
                            w2.scroll += 3;
                        }
                        g.wins[slot] = Some(w2);
                    }
                    _ => {}
                }
            }
            return false;
        }
    }
    false
}

/// 启动桌面。鼠标可用时以鼠标为主，键盘仍可操作。
pub fn run() {
    if !fb::available() {
        crate::println!("{}", crate::tr!(
            "Desktop needs a VBE framebuffer (text mode fallback active).",
            "桌面需要 VBE 帧缓冲（当前为文本模式回退）。"
        ));
        return;
    }

    let mut g = Gui::new();
    let mut kb = Keyboard::new();
    if !mouse::ready() { mouse::init(); }
    crate::cursor::init();
    crate::cursor::hide();
    draw_all(&g);
    crate::cursor::show();

    loop {
        let moved = mouse::update();

        // 拖动窗口
        if let Some((slot, ox, oy)) = g.drag {
            if mouse::left_down() {
                if moved {
                    if let Some(w) = g.wins[slot].as_mut() {
                        let sw = fb::width() as i32;
                        let sh = fb::height() as i32;
                        w.x = (mouse::x() - ox).clamp(4, sw - w.w - 4);
                        w.y = (mouse::y() - oy).clamp(PANEL_H + 2, sh - TASKBAR_H - w.h - 2);
                    }
                    crate::cursor::hide();
                    draw_all(&g);
                    crate::cursor::show();
                }
            } else {
                g.drag = None;
            }
        } else if moved {
            crate::cursor::sync();
            if g.menu_open {
                crate::cursor::hide();
                draw_all(&g);
                crate::cursor::show();
            }
        }

        if let Some((cx, cy, left)) = mouse::take_click() {
            if left {
                crate::cursor::hide();
                let exit = handle_click(&mut g, cx, cy);
                draw_all(&g);
                crate::cursor::show();
                if exit { break; }
            } else {
                g.menu_open = false;
                crate::cursor::hide();
                draw_all(&g);
                crate::cursor::show();
            }
        }

        let sc = match keyboard::read_scancode() {
            Some(s) => s,
            None => { unsafe { core::arch::asm!("pause"); } continue; }
        };
        let key = match kb.handle_scancode(sc) {
            Some(k) => k,
            None => continue,
        };

        let mut exit = false;
        let mut redraw = false;
        match key {
            Key::Esc => {
                if g.menu_open {
                    g.menu_open = false;
                } else if let Some(slot) = g.focused_slot() {
                    g.remove(slot);
                } else {
                    exit = true;
                }
                redraw = true;
            }
            Key::Tab => {
                if g.nwin > 1 {
                    g.raise(g.order[0]);
                }
                redraw = true;
            }
            Key::Up | Key::Down => {
                let down = matches!(key, Key::Down);
                if g.menu_open {
                    let n = MENU_LEN;
                    g.menu_sel = if down { (g.menu_sel + 1) % n } else { (g.menu_sel + n - 1) % n };
                } else if let Some(slot) = g.focused_slot() {
                    if let Some(mut w) = g.wins[slot] {
                        match w.app {
                            App::Files => {
                                let n = g.count_files();
                                if n > 0 {
                                    g.files_sel = if down { (g.files_sel + 1) % n } else { (g.files_sel + n - 1) % n };
                                }
                            }
                            App::View => {
                                w.scroll = if down { w.scroll + 1 } else { w.scroll.saturating_sub(1) };
                                g.wins[slot] = Some(w);
                            }
                            _ => {}
                        }
                    }
                }
                redraw = true;
            }
            Key::PageDown | Key::PageUp => {
                if let Some(slot) = g.focused_slot() {
                    if let Some(mut w) = g.wins[slot] {
                        if w.app == App::View {
                            if matches!(key, Key::PageDown) { w.scroll += 10; }
                            else { w.scroll = w.scroll.saturating_sub(10); }
                            g.wins[slot] = Some(w);
                        }
                    }
                }
                redraw = true;
            }
            Key::Enter => {
                if g.menu_open {
                    g.menu_open = false;
                    let sel = g.menu_sel;
                    exit = activate_menu(&mut g, sel);
                } else if let Some(slot) = g.focused_slot() {
                    if let Some(w) = g.wins[slot] {
                        if w.app == App::Files {
                            let fi = g.files_sel;
                            open_view(&mut g, fi);
                        }
                    }
                }
                redraw = true;
            }
            Key::Char(c) => match c {
                'm' | 'M' => { g.menu_open = !g.menu_open; redraw = true; }
                'l' | 'L' => { vga::set_lang(!vga::lang_zh()); redraw = true; }
                'd' | 'D' => {
                    if let Some(slot) = g.focused_slot() {
                        if let Some(w) = g.wins[slot] {
                            if w.app == App::Files {
                                if let Some(name) = g.nth_file(g.files_sel) {
                                    let _ = fs::delete(name);
                                    let n = g.count_files();
                                    if g.files_sel >= n && g.files_sel > 0 { g.files_sel = n.saturating_sub(1); }
                                }
                            }
                        }
                    }
                    redraw = true;
                }
                _ => {}
            },
            _ => {}
        }

        if exit { break; }
        if redraw {
            crate::cursor::hide();
            draw_all(&g);
            crate::cursor::show();
        }
    }

    crate::cursor::hide();
    vga::with_writer(|w| w.clear());
}