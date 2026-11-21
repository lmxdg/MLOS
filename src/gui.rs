//! MLOS 桌面（Linux 风格，支持鼠标点击）。
//!
//! 文本模式图形桌面：顶部面板 + 桌面图标 + 可平移焦点的窗口 + 底部任务栏。
//! 支持 PS/2 鼠标点击（打开菜单、启动程序、聚焦/关闭窗口、选择文件），
//! 同时保留键盘快捷键（Tab 切换窗口、M 菜单、方向键、Enter、Esc）。
//! 中文模式下所有标签通过点阵字模显示。

use crate::fs;
use crate::keyboard::{self, Key, Keyboard};
use crate::mouse;
use crate::perf;
use crate::vga::{self, Color};
use crate::TOTAL_MEMORY;
use alloc::{format, string::{String, ToString}};
use core::sync::atomic::Ordering;

const WIDTH: usize = 80;
const HEIGHT: usize = 25;
const TASKBAR_ROW: usize = HEIGHT - 1;
const PANEL_ROW: usize = 0;

#[derive(Clone, Copy, PartialEq, Eq)]
enum WinKind { Perf, Files, View, About, Usb }

#[derive(Clone, Copy, PartialEq, Eq)]
struct Window {
    kind: WinKind,
    title: &'static str,
    pos: usize,
}

/// 窗口固定在右侧 2x2 网格，左侧留给桌面图标
const WIN_POS: [(usize, usize); 4] = [(16, 2), (48, 2), (16, 13), (48, 13)];
const WIN_W: usize = 30;
const WIN_H: usize = 10;

const MENU_EN: [&str; 6] = [
    "Performance Monitor", "File Explorer", "USB Devices",
    "About MLOS", "Language en / zh", "Exit to Shell",
];
const MENU_ZH: [&str; 6] = [
    "性能监视器", "文件管理器", "USB 设备",
    "关于 MLOS", "切换语言 en / zh", "退出到命令行",
];
const ICON_EN: [&str; 5] = ["Perf", "Files", "USB", "About", "Exit"];
const ICON_ZH: [&str; 5] = ["性能", "文件", "USB", "关于", "退出"];

fn menu_label(i: usize) -> &'static str { crate::tr!(MENU_EN[i], MENU_ZH[i]) }
fn icon_label(i: usize) -> &'static str { crate::tr!(ICON_EN[i], ICON_ZH[i]) }

/// 全屏单元格网格：字符码 + 颜色（fg<<4 | bg）
struct Screen {
    ch: [[u8; WIDTH]; HEIGHT],
    col: [[u8; WIDTH]; HEIGHT],
}

impl Screen {
    fn new() -> Self {
        Screen {
            ch: [[b' '; WIDTH]; HEIGHT],
            col: [[(Color::Black.code() << 4) | Color::Blue.code(); WIDTH]; HEIGHT],
        }
    }

    fn put(&mut self, row: usize, col: usize, ch: u8, fg: Color, bg: Color) {
        if row >= HEIGHT || col >= WIDTH { return; }
        self.ch[row][col] = ch;
        self.col[row][col] = (fg.code() << 4) | bg.code();
    }

    /// 写入文本：ASCII 直写；UTF-8 汉字经字库映射为两个字符码
    fn text(&mut self, row: usize, col: usize, s: &str, fg: Color, bg: Color) {
        let bytes = s.as_bytes();
        let mut c = col;
        let mut i = 0usize;
        while i < bytes.len() {
            let b = bytes[i];
            if b < 0x80 {
                self.put(row, c, b, fg, bg);
                c += 1;
                i += 1;
            } else if b < 0xE0 {
                i += 2;
            } else if b < 0xF0 && i + 2 < bytes.len() {
                let cp = ((b as u32 & 0x0F) << 12)
                    | ((bytes[i + 1] as u32 & 0x3F) << 6)
                    | (bytes[i + 2] as u32 & 0x3F);
                if vga::lang_zh() {
                    match crate::zhfont::glyph_bytes(cp) {
                        Some((lo, hi)) => {
                            self.put(row, c, lo, fg, bg);
                            self.put(row, c + 1, hi, fg, bg);
                            c += 2;
                        }
                        None => { self.put(row, c, b'?', fg, bg); c += 1; }
                    }
                }
                i += 3;
            } else { i += 1; }
        }
    }

    fn fill(&mut self, row: usize, col: usize, len: usize, ch: u8, fg: Color, bg: Color) {
        for i in 0..len { self.put(row, col + i, ch, fg, bg); }
    }

    /// 把网格刷新到 VGA；鼠标光标以反色字符叠加
    fn flush(&self) {
        vga::with_writer(|w| {
            let (mc, mr) = (mouse::col(), mouse::row());
            for row in 0..HEIGHT {
                let mut col = 0usize;
                while col < WIDTH {
                    if row == mr && col == mc {
                        w.set_color(Color::Black, Color::White);
                        w.put_byte(b'+');
                        col += 1;
                        continue;
                    }
                    let c = self.col[row][col];
                    let fg = unsafe { core::mem::transmute::<u8, Color>((c >> 4) & 0x0F) };
                    let bg = unsafe { core::mem::transmute::<u8, Color>(c & 0x0F) };
                    w.set_color(fg, bg);
                    while col < WIDTH
                        && !(row == mr && col == mc)
                        && self.col[row][col] == c
                    {
                        w.put_byte(self.ch[row][col]);
                        col += 1;
                    }
                }
                w.put_byte(b'\n');
            }
        });
    }
}

struct Gui {
    wins: [Option<Window>; 4],
    focused: usize,
    menu_open: bool,
    menu_sel: usize,
    files_sel: usize,
    view_name: [u8; 30],
    view_len: usize,
}

/// 启动桌面。鼠标可用时以鼠标为主，键盘仍可操作。
pub fn run() {
    let mut g = Gui {
        wins: [None; 4],
        focused: 0,
        menu_open: false,
        menu_sel: 0,
        files_sel: 0,
        view_name: [0; 30],
        view_len: 0,
    };
    g.wins[0] = Some(Window {
        kind: WinKind::About,
        title: crate::tr!("Welcome to MLOS", "欢迎使用 MLOS"),
        pos: 0,
    });

    let mut kb = Keyboard::new();
    if !mouse::ready() { mouse::init(); }
    draw(&g);

    loop {
        mouse::update();

        // ---- 鼠标点击 ----
        if let Some((col, row, left)) = mouse::take_click() {
            if left && handle_click(&mut g, col, row) {
                break;
            }
            draw(&g);
        }

        // ---- 键盘 ----
        let sc = match keyboard::read_scancode() {
            Some(s) => s,
            None => {
                unsafe { core::arch::asm!("pause"); }
                continue;
            }
        };
        let key = match kb.handle_scancode(sc) {
            Some(k) => k,
            None => continue,
        };

        let mut exit = false;
        match key {
            Key::Esc => {
                if g.menu_open {
                    g.menu_open = false;
                } else if g.wins[g.focused].is_some() {
                    g.wins[g.focused] = None;
                    normalize_focus(&mut g);
                } else {
                    exit = true;
                }
            }
            Key::Tab => {
                if g.wins.iter().any(|w| w.is_some()) {
                    loop {
                        g.focused = (g.focused + 1) % 4;
                        if g.wins[g.focused].is_some() { break; }
                    }
                }
            }
            Key::Up | Key::Down => {
                let down = matches!(key, Key::Down);
                if g.menu_open {
                    let n = MENU_ITEMS_LEN;
                    g.menu_sel = if down { (g.menu_sel + 1) % n } else { (g.menu_sel + n - 1) % n };
                } else if let Some(win) = g.wins[g.focused] {
                    if win.kind == WinKind::Files { move_file_sel(&mut g, down); }
                }
            }
            Key::Left | Key::Right => {}
            Key::Enter => {
                if g.menu_open {
                    g.menu_open = false;
                    let sel = g.menu_sel;
                    if activate_menu(&mut g, sel) { exit = true; }
                } else if let Some(win) = g.wins[g.focused] {
                    if win.kind == WinKind::Files {
                        if let Some(name) = nth_file(g.files_sel) {
                            set_view_name(&mut g, name);
                            open_window(&mut g, WinKind::View,
                                crate::tr!("File Viewer", "文件查看器"));
                        }
                    }
                }
            }
            Key::Char(c) => match c {
                'm' | 'M' => g.menu_open = !g.menu_open,
                'd' | 'D' => {
                    if let Some(win) = g.wins[g.focused] {
                        if win.kind == WinKind::Files {
                            if let Some(name) = nth_file(g.files_sel) {
                                let _ = fs::delete(name);
                                let (_, count) = fs::usage();
                                if g.files_sel >= count as usize && g.files_sel > 0 {
                                    g.files_sel = (count as usize).saturating_sub(1);
                                }
                            }
                        }
                    }
                }
                _ => {}
            },
            Key::Backspace => {}
        }
        if exit { break; }
        draw(&g);
    }

    vga::with_writer(|w| w.clear());
}

const MENU_ITEMS_LEN: usize = MENU_EN.len();

fn move_file_sel(g: &mut Gui, down: bool) {
    let (_, count) = fs::usage();
    let count = count as usize;
    if count > 0 {
        g.files_sel = if down {
            (g.files_sel + 1) % count
        } else {
            (g.files_sel + count - 1) % count
        };
    }
}

/// 鼠标点击命中检测。返回 true 表示退出桌面。
fn handle_click(g: &mut Gui, col: usize, row: usize) -> bool {
    // 顶部面板按钮
    if row == PANEL_ROW {
        if (20..26).contains(&col) { g.menu_open = !g.menu_open; return false; }
        if (28..34).contains(&col) {
            if vga::lang_zh() { vga::set_lang(false); } else { vga::set_lang(true); }
            return false;
        }
    }

    // 下拉菜单
    if g.menu_open {
        let top = PANEL_ROW + 1;
        let w = 24;
        if col < w && row > top && row <= top + MENU_ITEMS_LEN {
            let idx = row - top - 1;
            g.menu_open = false;
            return activate_menu(g, idx);
        }
        // 点击菜单外区域关闭菜单
        g.menu_open = false;
    }

    // 任务栏：Start 按钮 + 窗口按钮
    if row == TASKBAR_ROW {
        if (1..8).contains(&col) { g.menu_open = true; return false; }
        let mut cx = 10usize;
        for (i, win) in g.wins.iter().enumerate() {
            if let Some(w2) = win {
                let t = format!("[{}]", w2.title);
                if col >= cx && col < cx + t.len().min(20) {
                    g.focused = i;
                    return false;
                }
                cx += t.len().min(20) + 2;
            }
        }
        return false;
    }

    // 桌面图标（左侧列）
    for i in 0..5 {
        let r = 2 + i * 2;
        if row == r && (2..14).contains(&col) {
            return activate_icon(g, i);
        }
    }

    // 窗口：关闭按钮、标题栏、内容
    for slot in 0..4 {
        if let Some(win) = g.wins[slot] {
            let (x, y) = WIN_POS[slot];
            if row == y && col >= x && col < x + WIN_W {
                if col >= x + WIN_W - 4 && col < x + WIN_W - 1 {
                    g.wins[slot] = None;
                    normalize_focus(g);
                } else {
                    g.focused = slot;
                }
                return false;
            }
            if row > y && row < y + WIN_H && col >= x && col < x + WIN_W {
                g.focused = slot;
                if win.kind == WinKind::Files {
                    let body_row = row - y;
                    if body_row >= 3 {
                        let idx = body_row - 3;
                        let (_, count) = fs::usage();
                        if idx < count as usize {
                            if let Some(name) = nth_file(idx) {
                                g.files_sel = idx;
                                set_view_name(g, name);
                                open_window(g, WinKind::View,
                                    crate::tr!("File Viewer", "文件查看器"));
                            }
                        }
                    }
                }
                return false;
            }
        }
    }
    false
}

fn activate_icon(g: &mut Gui, i: usize) -> bool {
    match i {
        0 => open_window(g, WinKind::Perf, crate::tr!("Performance", "性能监视器")),
        1 => open_window(g, WinKind::Files, crate::tr!("Files", "文件管理器")),
        2 => open_window(g, WinKind::Usb, crate::tr!("USB Devices", "USB 设备")),
        3 => open_window(g, WinKind::About, crate::tr!("About MLOS", "关于 MLOS")),
        _ => return true,
    }
    false
}

fn activate_menu(g: &mut Gui, idx: usize) -> bool {
    match idx {
        0 => open_window(g, WinKind::Perf, crate::tr!("Performance Monitor", "性能监视器")),
        1 => open_window(g, WinKind::Files, crate::tr!("File Explorer", "文件管理器")),
        2 => open_window(g, WinKind::Usb, crate::tr!("USB Devices", "USB 设备")),
        3 => open_window(g, WinKind::About, crate::tr!("About MLOS", "关于 MLOS")),
        4 => {
            if vga::lang_zh() { vga::set_lang(false); } else { vga::set_lang(true); }
        }
        _ => return true,
    }
    false
}

fn normalize_focus(g: &mut Gui) {
    if g.wins[g.focused].is_none() {
        for i in 0..4 {
            if g.wins[i].is_some() { g.focused = i; return; }
        }
        g.focused = 0;
    }
}

fn open_window(g: &mut Gui, kind: WinKind, title: &'static str) {
    for pos in 0..4 {
        let occupied = g.wins.iter().flatten().any(|w| w.pos == pos);
        if !occupied {
            for slot in 0..4 {
                if g.wins[slot].is_none() {
                    g.wins[slot] = Some(Window { kind, title, pos });
                    g.focused = slot;
                    return;
                }
            }
        }
    }
    let pos = g.wins[g.focused].as_ref().map(|w| w.pos).unwrap_or(0);
    g.wins[g.focused] = Some(Window { kind, title, pos });
}

fn set_view_name(g: &mut Gui, name: &str) {
    g.view_name = [0; 30];
    let n = name.len().min(29);
    g.view_name[..n].copy_from_slice(&name.as_bytes()[..n]);
    g.view_len = n;
}

fn view_name_str(g: &Gui) -> &str {
    core::str::from_utf8(&g.view_name[..g.view_len]).unwrap_or("")
}

fn nth_file(i: usize) -> Option<&'static str> {
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

// ---------------- drawing ----------------

fn usb_line(r: usize) -> Option<String> {
    let n = crate::usb::count();
    let text = match r {
        1 => {
            if !crate::usb::is_scanned() {
                crate::tr!(" Bus not scanned yet.", " 总线尚未扫描。").to_string()
            } else if n == 0 {
                crate::tr!(" No USB host controller.", " 未检测到 USB 主控。").to_string()
            } else {
                format!(" {} controller(s):", n)
            }
        }
        _ => {
            let mut idx = 0usize;
            let mut out: Option<String> = None;
            crate::usb::each(|c| {
                idx += 1;
                let row = 3 + (idx - 1) * 2;
                if row == r + 1 {
                    out = Some(format!(" [{} {:X}:{:X}] {}", c.kind, c.bus, c.device, c.speed.label()));
                }
            });
            match out {
                Some(t) => t,
                None => {
                    if r == 7 {
                        crate::tr!(" USB 1.0 - 3.2 compatible", " 兼容 USB 1.0 - 3.2").to_string()
                    } else { return None; }
                }
            }
        }
    };
    Some(text)
}

fn draw(g: &Gui) {
    let mut scr = Screen::new();

    // 顶部面板
    scr.fill(PANEL_ROW, 0, WIDTH, b' ', Color::White, Color::Blue);
    scr.text(PANEL_ROW, 1, crate::tr!("MLOS Desktop", "MLOS 桌面"), Color::White, Color::Blue);
    scr.text(PANEL_ROW, 20, crate::tr!("[Apps]", "[应用]"), Color::Yellow, Color::Blue);
    scr.text(PANEL_ROW, 28, "[EN/中]", Color::LightGreen, Color::Blue);
    let lang = if vga::lang_zh() { "中文预览" } else { "English" };
    scr.text(PANEL_ROW, 50, lang, Color::LightCyan, Color::Blue);
    let ver = format!("v1.0.6 USB:{}", crate::usb::count());
    scr.text(PANEL_ROW, WIDTH - ver.len() - 1, &ver, Color::White, Color::Blue);

    // 桌面图标
    for i in 0..5 {
        let r = 2 + i * 2;
        scr.text(r, 2, "[ ]", Color::White, Color::Blue);
        scr.text(r, 3, icon_label(i), Color::Yellow, Color::Blue);
    }

    // 窗口
    for (i, win) in g.wins.iter().enumerate() {
        if let Some(win) = win { draw_window(&mut scr, g, win, i == g.focused); }
    }

    // 下拉菜单
    if g.menu_open {
        let top = PANEL_ROW + 1;
        let w = 24;
        scr.fill(top, 0, w, b'-', Color::White, Color::Blue);
        for i in 0..MENU_ITEMS_LEN {
            let row = top + 1 + i;
            let (fg, bg) = if i == g.menu_sel {
                (Color::White, Color::Green)
            } else {
                (Color::White, Color::Blue)
            };
            scr.fill(row, 0, w, b' ', fg, bg);
            scr.text(row, 1, menu_label(i), fg, bg);
        }
        scr.fill(top + MENU_ITEMS_LEN + 1, 0, w, b'-', Color::White, Color::Blue);
    }

    // 任务栏
    scr.fill(TASKBAR_ROW, 0, WIDTH, b' ', Color::LightGray, Color::DarkGray);
    let (sfg, sbg) = if g.menu_open { (Color::White, Color::Brown) } else { (Color::White, Color::Green) };
    scr.text(TASKBAR_ROW, 1, crate::tr!("[Start]", "[开始]"), sfg, sbg);
    let mut cx = 10usize;
    for (i, win) in g.wins.iter().enumerate() {
        if let Some(w2) = win {
            let (fg, bg) = if i == g.focused {
                (Color::White, Color::Blue)
            } else {
                (Color::LightGray, Color::DarkGray)
            };
            let t = format!("[{}]", w2.title);
            if cx + t.len() < WIDTH - 20 {
                scr.text(TASKBAR_ROW, cx, &t, fg, bg);
                cx += t.len() + 2;
            }
        }
    }
    let hint = crate::tr!("Click to open | Esc quit", "点击打开 | Esc 退出");
    scr.text(TASKBAR_ROW, WIDTH - hint.len() - 1, hint, Color::Yellow, Color::DarkGray);

    scr.flush();
}

fn draw_window(scr: &mut Screen, g: &Gui, win: &Window, focused: bool) {
    let (x, y) = WIN_POS[win.pos];
    let (t_fg, t_bg) = if focused {
        (Color::White, Color::Blue)
    } else {
        (Color::Black, Color::LightGray)
    };
    let (b_fg, b_bg) = (Color::LightGray, Color::Black);
    let bd = if focused { Color::White } else { Color::DarkGray };

    // 标题栏（ASCII 边框，避免与中文点阵字模冲突）
    scr.fill(y, x + 1, WIN_W - 2, b'-', t_fg, t_bg);
    scr.put(y, x, b'+', t_fg, t_bg);
    scr.put(y, x + WIN_W - 1, b'+', t_fg, t_bg);
    let label = format!(" {} ", win.title);
    scr.text(y, x + 2, &label, t_fg, t_bg);
    scr.text(y, x + WIN_W - 4, "[X]", Color::Yellow, t_bg);

    for r in 1..WIN_H - 1 {
        scr.fill(y + r, x + 1, WIN_W - 2, b' ', b_fg, b_bg);
        scr.put(y + r, x, b'|', bd, b_bg);
        scr.put(y + r, x + WIN_W - 1, b'|', bd, b_bg);
        let inner = WIN_W - 4;
        let line = match win.kind {
            WinKind::Perf => perf_line(r - 1),
            WinKind::About => about_line(r - 1),
            WinKind::Files => files_line(g, r - 1),
            WinKind::View => view_line(g, r - 1, inner),
            WinKind::Usb => usb_line(r - 1),
        };
        if let Some(text) = line {
            let t: String = text.chars().take(inner).collect();
            scr.text(y + r, x + 2, &t, b_fg, b_bg);
        }
    }
    scr.put(y + WIN_H - 1, x, b'+', bd, b_bg);
    scr.fill(y + WIN_H - 1, x + 1, WIN_W - 2, b'-', bd, b_bg);
    scr.put(y + WIN_H - 1, x + WIN_W - 1, b'+', bd, b_bg);
}

fn perf_line(r: usize) -> Option<String> {
    if r > 8 { return None; }
    let cpu = perf::cpu_info();
    let vendor = core::str::from_utf8(&cpu.vendor[..12]).unwrap_or("unknown");
    let mem = perf::mem_info();
    let disk = perf::disk_info();
    let text = match r {
        1 => format!(" CPU    : {}", vendor),
        2 => format!(" TSC    : ~{} MHz", cpu.tsc_hz / 1_000_000),
        3 => format!(" MEM    : {} MiB", mem.total_kib / 1024),
        4 => format!(" Kernel : ~{} KiB", mem.kernel_kib),
        5 => format!(" Usage  : {}%", ((mem.kernel_kib * 100) / mem.total_kib.max(1)).min(100)),
        7 => format!(" Disk   : {}/{} sec", disk.used_sectors, disk.total_sectors),
        _ => return None,
    };
    Some(text)
}

fn about_line(r: usize) -> Option<String> {
    let text = match r {
        1 => "MLOS - Minimal Light OS",
        2 => "v1.0.6  x86_64  Rust no_std",
        3 => "Developer: MEME-LMXDG",
        5 => crate::tr!("Mouse + keyboard desktop", "支持鼠标与键盘的桌面"),
        6 => crate::tr!("Click icons / menus to start", "点击图标或菜单启动程序"),
        8 => crate::tr!("Esc: close / quit", "Esc: 关闭/退出"),
        _ => return None,
    };
    Some(text.to_string())
}

fn files_line(g: &Gui, r: usize) -> Option<String> {
    if r == 0 {
        return Some(crate::tr!("Click a file to view", "点击文件查看").into());
    }
    let (_, count) = fs::usage();
    let count = count as usize;
    if r == 2 && count == 0 {
        return Some(crate::tr!("(empty - use 'write')", "（空，请用 write 命令）").into());
    }
    if r < 3 { return None; }
    let idx = r - 3;
    if idx >= 12 { return None; }
    let name = nth_file(idx)?;
    let cur = if idx == g.files_sel && idx < count { "> " } else { "  " };
    let mut size = 0u32;
    if let Some((entries, _)) = fs::list() {
        for e in entries.into_iter().flatten() {
            if e.name_str() == name { size = e.size; }
        }
    }
    Some(format!(" {}{:<20} {:>4} B", cur, name, size))
}

fn view_line(g: &Gui, r: usize, inner: usize) -> Option<String> {
    if r == 0 {
        return Some(format!("File: {}  [Esc]", view_name_str(g)));
    }
    static mut CACHE: ([u8; 512], usize, [u8; 30], usize) = ([0; 512], 0, [0; 30], 0);
    unsafe {
        if CACHE.2 != g.view_name {
            let mut buf = [0u8; 512];
            let n = fs::read(view_name_str(g), &mut buf).unwrap_or(0);
            CACHE = (buf, n, g.view_name, 1);
        }
        let (buf, n, _, _) = CACHE;
        let line_idx = r - 1;
        if line_idx > 6 { return None; }
        let per = inner.saturating_sub(1);
        let start = line_idx * per;
        if start >= n { return None; }
        let end = (start + per).min(n);
        let txt = core::str::from_utf8(&buf[start..end]).unwrap_or("");
        Some(format!(" {}", txt))
    }
}

#[allow(dead_code)]
fn total_mem_kib() -> u64 {
    TOTAL_MEMORY.load(Ordering::Relaxed)
}
