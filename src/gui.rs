//! Experimental Windows-like GUI for MLOS (text-mode "desktop").
//!
//! Keyboard-driven desktop drawn on the VGA text screen: wallpaper, taskbar
//! with a Start menu, tiled windows (Performance Monitor / File Explorer /
//! File Viewer / About). Started only with `gui on` after an explicit
//! experimental-feature warning; Esc closes windows and exits to the shell.

use crate::fs;
use alloc::{format, string::{String, ToString}};
use crate::keyboard::{self, Key, Keyboard};
use crate::perf;
use crate::vga::{self, Color};
use crate::TOTAL_MEMORY;
use core::sync::atomic::Ordering;

const WIDTH: usize = 80;
const HEIGHT: usize = 25;
const TASKBAR_ROW: usize = HEIGHT - 1;

#[derive(Clone, Copy, PartialEq, Eq)]
enum WinKind {
    Perf,
    Files,
    View,
    About,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Window {
    kind: WinKind,
    title: &'static str,
    /// grid slot: 0..=3 (2x2 tiled layout)
    pos: usize,
}

// window geometry
const WIN_X: [usize; 4] = [1, 41, 1, 41];
const WIN_Y: [usize; 4] = [1, 1, 13, 13];
const WIN_W: usize = 38;
const WIN_H: usize = 11;

/// A full-screen cell grid: character + color pair.
struct Screen {
    ch: [[u8; WIDTH]; HEIGHT],
    col: [[u8; WIDTH]; HEIGHT], // fg<<4 | bg
}

impl Screen {
    fn new() -> Self {
        Screen { ch: [[b' '; WIDTH]; HEIGHT], col: [[(Color::Black.code() << 4) | Color::Cyan.code(); WIDTH]; HEIGHT] }
    }

    fn put(&mut self, row: usize, col: usize, ch: u8, fg: Color, bg: Color) {
        if row >= HEIGHT || col >= WIDTH {
            return;
        }
        self.ch[row][col] = ch;
        self.col[row][col] = (fg.code() << 4) | bg.code();
    }

    fn text(&mut self, row: usize, col: usize, s: &str, fg: Color, bg: Color) {
        for (i, &b) in s.as_bytes().iter().enumerate() {
            self.put(row, col + i, b, fg, bg);
        }
    }

    fn fill(&mut self, row: usize, col: usize, len: usize, ch: u8, fg: Color, bg: Color) {
        for i in 0..len {
            self.put(row, col + i, ch, fg, bg);
        }
    }

    /// Flush the grid to the VGA console (color runs are merged).
    fn flush(&self) {
        vga::with_writer(|w| {
            for row in 0..HEIGHT {
                let mut col = 0usize;
                while col < WIDTH {
                    let c = self.col[row][col];
                    let fg = unsafe { core::mem::transmute::<u8, Color>((c >> 4) & 0x0F) };
                    let bg = unsafe { core::mem::transmute::<u8, Color>(c & 0x0F) };
                    w.set_color(fg, bg);
                    let start = col;
                    while col < WIDTH && self.col[row][col] == c {
                        w.put_byte(self.ch[row][col]);
                        col += 1;
                    }
                    let _ = start;
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

const MENU_ITEMS: [&str; 4] = [
    "Performance Monitor",
    "File Explorer",
    "About MLOS",
    "Exit to Shell",
];

/// Entry point after the experimental confirmation. Returns on exit.
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
    g.wins[0] = Some(Window { kind: WinKind::About, title: "Welcome - MLOS", pos: 0 });
    g.focused = 0;

    let mut kb = Keyboard::new();
    draw(&g);

    loop {
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
                        if g.wins[g.focused].is_some() {
                            break;
                        }
                    }
                }
            }
            Key::Up | Key::Down => {
                let down = matches!(key, Key::Down);
                if g.menu_open {
                    let n = MENU_ITEMS.len();
                    g.menu_sel = if down {
                        (g.menu_sel + 1) % n
                    } else {
                        (g.menu_sel + n - 1) % n
                    };
                } else if let Some(win) = g.wins[g.focused] {
                    if win.kind == WinKind::Files {
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
                }
            }
            Key::Left | Key::Right => {}
            Key::Enter => {
                if g.menu_open {
                    g.menu_open = false;
                    match g.menu_sel {
                        0 => open_window(&mut g, WinKind::Perf, "Performance Monitor"),
                        1 => open_window(&mut g, WinKind::Files, "File Explorer"),
                        2 => open_window(&mut g, WinKind::About, "About MLOS"),
                        _ => exit = true,
                    }
                } else if let Some(win) = g.wins[g.focused] {
                    if win.kind == WinKind::Files {
                        if let Some(name) = nth_file(g.files_sel) {
                            set_view_name(&mut g, name);
                            open_window(&mut g, WinKind::View, "File Viewer");
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
                        let count = count as usize;
                                if g.files_sel >= count && g.files_sel > 0 {
                                    g.files_sel = count.saturating_sub(1);
                                }
                            }
                        }
                    }
                }
                _ => {}
            },
            Key::Backspace => {}
        }

        if exit {
            break;
        }
        draw(&g);
    }
}

fn normalize_focus(g: &mut Gui) {
    if g.wins[g.focused].is_none() {
        for i in 0..4 {
            if g.wins[i].is_some() {
                g.focused = i;
                return;
            }
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

/// Return the i-th file name (cached per call to satisfy the borrow checker).
fn nth_file(i: usize) -> Option<&'static str> {
    static mut NAMES: [[u8; 30]; 12] = [[0; 30]; 12];
    static mut COUNT: usize = 0;
    unsafe {
        let (entries, count) = fs::list()?;
        COUNT = count;
        for (idx, e) in entries.iter().flatten().enumerate() {
            NAMES[idx] = e.name;
        }
        if i < COUNT {
            let len = NAMES[i].iter().position(|&b| b == 0).unwrap_or(30);
            Some(core::str::from_utf8(&NAMES[i][..len]).unwrap_or(""))
        } else {
            None
        }
    }
}

// ---------------- drawing ----------------

fn draw(g: &Gui) {
    let mut scr = Screen::new();

    // windows (tiled)
    for (i, win) in g.wins.iter().enumerate() {
        if let Some(win) = win {
            draw_window(&mut scr, g, win, i == g.focused);
        }
    }

    // start menu overlay (above the taskbar, bottom-left)
    if g.menu_open {
        let top = TASKBAR_ROW - MENU_ITEMS.len() - 2;
        let w = 24;
        scr.fill(top, 0, w, b'+', Color::White, Color::Blue);
        for i in 0..MENU_ITEMS.len() {
            let row = top + 1 + i;
            let (fg, bg) = if i == g.menu_sel {
                (Color::White, Color::Green)
            } else {
                (Color::White, Color::Blue)
            };
            scr.fill(row, 0, w, b' ', fg, bg);
            scr.text(row, 1, MENU_ITEMS[i], fg, bg);
        }
        scr.fill(top + MENU_ITEMS.len() + 1, 0, w, b'+', Color::White, Color::Blue);
    }

    // taskbar
    let tb_bg = Color::DarkGray;
    scr.fill(TASKBAR_ROW, 0, WIDTH, b' ', Color::LightGray, tb_bg);
    let (sfg, sbg) = if g.menu_open {
        (Color::White, Color::Brown)
    } else {
        (Color::White, Color::Green)
    };
    scr.text(TASKBAR_ROW, 1, "[Start]", sfg, sbg);
    let mut cx = 10usize;
    for (i, win) in g.wins.iter().enumerate() {
        if let Some(w2) = win {
            let (fg, bg) = if i == g.focused {
                (Color::White, Color::Blue)
            } else {
                (Color::LightGray, Color::DarkGray)
            };
            let t = format!("[{}]", w2.title);
            if cx + t.len() < WIDTH - 22 {
                scr.text(TASKBAR_ROW, cx, &t, fg, bg);
                cx += t.len() + 2;
            }
        }
    }
    scr.text(TASKBAR_ROW, WIDTH - 21, "MLOS GUI [EXPERIMENTAL]", Color::Yellow, tb_bg);

    scr.flush();
}

fn draw_window(scr: &mut Screen, g: &Gui, win: &Window, focused: bool) {
    let (x, y) = (WIN_X[win.pos], WIN_Y[win.pos]);
    let (t_fg, t_bg) = if focused {
        (Color::White, Color::Blue)
    } else {
        (Color::Black, Color::LightGray)
    };
    let (b_fg, b_bg) = (Color::LightGray, Color::Black);
    let bd = if focused { Color::White } else { Color::DarkGray };

    // title bar
    scr.fill(y, x, WIN_W, b'-', t_fg, t_bg);
    let label = format!("[ {} ]", win.title);
    scr.text(y, x + 1, &label, t_fg, t_bg);

    // body frame
    for r in 1..WIN_H - 1 {
        scr.fill(y + r, x, WIN_W, b' ', b_fg, b_bg);
        scr.put(y + r, x, b'|', bd, b_bg);
        scr.put(y + r, x + WIN_W - 1, b'|', bd, b_bg);
        // content line
        let inner = WIN_W - 4;
        let line = match win.kind {
            WinKind::Perf => perf_line(r - 1, inner),
            WinKind::About => about_line(r - 1),
            WinKind::Files => files_line(g, r - 1),
            WinKind::View => view_line(g, r - 1, inner),
        };
        if let Some(text) = line {
            let t: String = text.chars().take(inner).collect();
            scr.text(y + r, x + 2, &t, b_fg, b_bg);
        }
    }
    // bottom border
    scr.fill(y + WIN_H - 1, x, WIN_W, b'-', bd, b_bg);
}

fn perf_line(r: usize, _inner: usize) -> Option<String> {
    if r > 9 {
        return None;
    }
    let cpu = perf::cpu_info();
    let vendor = core::str::from_utf8(&cpu.vendor[..12]).unwrap_or("unknown");
    let mem = perf::mem_info();
    let disk = perf::disk_info();
    let text = match r {
        1 => format!(" CPU    : {}", vendor),
        2 => format!(" TSC    : ~{} MHz", cpu.tsc_hz / 1_000_000),
        3 => format!(" Cycles : {}", cpu.tsc_now),
        5 => format!(" Memory : {} MiB usable", mem.total_kib / 1024),
        6 => format!(" Kernel : ~{} KiB | Free: {} KiB", mem.kernel_kib, mem.free_kib),
        7 => format!(" Usage  : {}%", ((mem.kernel_kib * 100) / mem.total_kib.max(1)).min(100)),
        9 => format!(" Disk   : {} / {} sectors used", disk.used_sectors, disk.total_sectors),
        _ => return None,
    };
    Some(text)
}

fn about_line(r: usize) -> Option<String> {
    let text = match r {
        1 => "MLOS - Minimal Light Operating System",
        2 => "Developer: MEME-LMXDG",
        3 => "Rust no_std kernel for x86_64",
        5 => "This GUI is an EXPERIMENTAL feature.",
        6 => "Keys: Tab focus | M menu | Esc close",
        8 => "Press M to open the Start menu.",
        _ => return None,
    };
    Some(text.to_string())
}

fn files_line(g: &Gui, r: usize) -> Option<String> {
    if r == 0 {
        return Some("Up/Down select | Enter view | D delete".into());
    }
    let (_, count) = fs::usage();
                        let count = count as usize;
    if r == 2 && count == 0 {
        return Some("(no files - use shell: write <name> <text>)".into());
    }
    if r < 3 {
        return None;
    }
    let idx = r - 3;
    if idx >= 12 {
        return None;
    }
    let name = nth_file(idx)?;
    let cur = if idx == g.files_sel && idx < count { "> " } else { "  " };
    let mut size = 0u32;
    if let Some((entries, _)) = fs::list() {
        for e in entries.into_iter().flatten() {
            if e.name_str() == name {
                size = e.size;
            }
        }
    }
    Some(format!(" {}{:<26} {:>5} B", cur, name, size))
}

fn view_line(g: &Gui, r: usize, inner: usize) -> Option<String> {
    if r == 0 {
        return Some(format!("File: {}  (Esc closes)", view_name_str(g)));
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
        if line_idx > 7 {
            return None;
        }
        let per = inner - 2;
        let start = line_idx * per;
        if start >= n {
            return None;
        }
        let end = (start + per).min(n);
        let txt = core::str::from_utf8(&buf[start..end]).unwrap_or("");
        Some(format!(" {}", txt))
    }
}

/// Wrapper so the GUI module references TOTAL_MEMORY (kept for future use).
#[allow(dead_code)]
fn total_mem_kib() -> u64 {
    TOTAL_MEMORY.load(Ordering::Relaxed)
}
