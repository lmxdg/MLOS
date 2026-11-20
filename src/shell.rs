//! 交互式命令行 Shell（MLOS）

use crate::games;
use crate::input::{read_line, wait_key};
use crate::keyboard::{self, Key, Keyboard};
use crate::storage::Config;
use crate::tools;
use crate::vga::{self, Color};
use crate::{halt_loop, println, print, TOTAL_MEMORY};
use core::fmt::Write;
use core::sync::atomic::Ordering;

const INPUT_CAP: usize = 128;

/// 当前登录的用户名副本
static mut CURRENT_USER: [u8; 32] = [0; 32];

pub fn prompt() {
    let user = current_user();
    vga::with_writer(|w| {
        w.set_color(Color::LightGreen, Color::Black);
        let _ = w.write_fmt(format_args!("{}", user));
        w.set_color(Color::DarkGray, Color::Black);
        w.put_str("@");
        w.set_color(Color::LightCyan, Color::Black);
        w.put_str("mlos");
        w.set_color(Color::DarkGray, Color::Black);
        w.put_str("$ ");
        w.set_color(Color::White, Color::Black);
        w.update_cursor();
    });
}

unsafe fn set_current_user(cfg: &Config) {
    CURRENT_USER = cfg.username;
}

fn current_user() -> &'static str {
    unsafe {
        let len = CURRENT_USER.iter().position(|&b| b == 0).unwrap_or(32);
        core::str::from_utf8(&CURRENT_USER[..len]).unwrap_or("user")
    }
}

pub fn run(cfg: &Config) -> ! {
    unsafe { set_current_user(cfg); }

    vga::with_writer(|w| w.clear());
    println!("MLOS v1.0.0 - Type 'help' for commands, 'games' or 'tools' for menus.\n");
    prompt();

    let mut kb = Keyboard::new();
    let mut buf = [0u8; INPUT_CAP];
    let mut len = 0usize;

    loop {
        if let Some(sc) = keyboard::read_scancode() {
            if let Some(key) = kb.handle_scancode(sc) {
                match key {
                    Key::Char(ch) if ch.is_ascii() => {
                        if len < INPUT_CAP - 1 {
                            buf[len] = ch as u8;
                            len += 1;
                            vga::with_writer(|w| w.put_byte(ch as u8));
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
                        let line = core::str::from_utf8(&buf[..len]).unwrap_or("");
                        execute(line, cfg);
                        len = 0;
                        prompt();
                    }
                    _ => {}
                }
            }
        }
        unsafe { core::arch::asm!("pause"); }
    }
}

fn execute(line: &str, cfg: &Config) {
    let line = line.trim();
    if line.is_empty() {
        return;
    }
    let cmd_end = line.find(char::is_whitespace).unwrap_or(line.len());
    let cmd = &line[..cmd_end];
    let rest = line[cmd_end..].trim();

    match cmd {
        // 基础命令
        "help" | "?" => cmd_help(),
        "clear" | "cls" => vga::with_writer(|w| w.clear()),
        "echo" => println!("{}", rest),
        "version" => println!("MLOS version 1.0.0 (x86_64)"),
        "about" => cmd_about(),
        "whoami" => println!("{}", current_user()),
        "uname" => println!("MLOS 1.0.0 mlos-host x86_64 GNU/Rust"),
        "date" => println!("Build time: {}", env!("BUILD_TIME")),
        "mem" => cmd_mem(),
        "color" => cmd_color(rest),
        "banner" => crate::print_banner(),
        "reboot" => reboot(),
        "halt" | "shutdown" => {
            println!("System halted. You may now power off.");
            halt_loop();
        }

        // 游戏
        "games" => game_menu(rest),
        "bomb" => games::number_bomb(),
        "guess" => games::guess_number(),
        "rps" => games::rock_paper_scissors(),

        // 工具
        "tools" => tool_menu(rest),
        "calc" => {
            if rest.is_empty() {
                tools::calculator_interactive();
            } else {
                tools::calculator(rest);
            }
        }
        "base" => tools::base_convert(rest),
        "prime" => tools::prime_check(rest),
        "hash" => tools::hash_text(rest),
        "rand" => tools::random_number(rest),
        "ascii" => tools::ascii_table(),

        // 磁盘/系统
        "disk" => cmd_disk(),
        "setup" => cmd_setup(cfg),
        _ => println!("mlos: command not found: {}\nType 'help' for available commands.", cmd),
    }
}

fn cmd_help() {
    println!("");
    vga::with_writer(|w| {
        w.set_color(Color::LightGreen, Color::Black);
        w.put_str("  === MLOS Commands ===\n");
        w.set_color(Color::White, Color::Black);
    });
    println!("  help               Show this help message");
    println!("  clear              Clear the screen");
    println!("  echo <text>        Print text");
    println!("  version            Show OS version");
    println!("  about              About MLOS");
    println!("  whoami             Show current user");
    println!("  uname              Show system information");
    println!("  date               Show build time");
    println!("  mem                Show usable memory");
    println!("  disk               Show hard disk status");
    println!("  color <fg> <bg>    Set text color (0-15)");
    println!("  banner             Redraw the boot banner");
    println!("  setup              Re-run setup wizard");
    println!("  reboot             Reboot the system");
    println!("  halt               Halt the system");
    println!("");
    println!("  GAMES:  games | bomb | guess | rps");
    println!("  TOOLS:  tools | calc | base | prime | hash | rand | ascii");
}

fn cmd_about() {
    println!("MLOS - Minimal Light Operating System written in Rust");
    println!("  - Arch    : x86_64 (64-bit)");
    println!("  - Boot    : GRUB 2 (Multiboot2)");
    println!("  - Display : VGA text mode 80x25");
    println!("  - Input   : PS/2 keyboard");
    println!("  - Storage : ATA PIO hard disk (persistent config)");
    println!("  - Core    : no_std bare-metal kernel");
}

fn cmd_mem() {
    let kib = TOTAL_MEMORY.load(Ordering::Relaxed);
    if kib == 0 {
        println!("Memory: unknown");
    } else {
        println!("Usable memory: {} KiB ({} MiB)", kib, kib / 1024);
    }
}

fn cmd_disk() {
    println!("");
    vga::with_writer(|w| {
        w.set_color(Color::LightBlue, Color::Black);
        w.put_str("  === Hard Disk Status ===\n");
        w.set_color(Color::White, Color::Black);
    });
    if crate::ata::detect() {
        println!("  Device      : Primary Master (ATA PIO)");
        println!("  Config LBA  : 1");
        match crate::storage::load() {
            Some(cfg) => {
                println!("  Config      : VALID (installed={})", cfg.installed);
                println!("  Username    : {}", cfg.user_str());
            }
            None => println!("  Config      : not found / invalid"),
        }
    } else {
        println!("  No hard disk detected.");
    }
}

fn cmd_setup(_cfg: &Config) {
    println!("Re-running setup wizard...");
    println!("(This will overwrite current user data.)");
    print!("Type 'yes' to continue: ");
    let mut buf = [0u8; 16];
    let n = read_line(&mut buf, None);
    let ans = core::str::from_utf8(&buf[..n]).unwrap_or("").trim();
    if ans.eq_ignore_ascii_case("yes") {
        match crate::setup::run_wizard() {
            Some(new_cfg) => {
                unsafe { set_current_user(&new_cfg); }
                println!("Setup complete. User updated.");
            }
            None => println!("Setup aborted."),
        }
    } else {
        println!("Cancelled.");
    }
}

fn game_menu(rest: &str) {
    if rest.is_empty() {
        games::menu();
        let mut buf = [0u8; 16];
        print!("  Select> ");
        let n = read_line(&mut buf, None);
        let sel = core::str::from_utf8(&buf[..n]).unwrap_or("").trim();
        match sel {
            "bomb" => games::number_bomb(),
            "guess" => games::guess_number(),
            "rps" => games::rock_paper_scissors(),
            _ => {}
        }
    } else {
        match rest {
            "bomb" => games::number_bomb(),
            "guess" => games::guess_number(),
            "rps" => games::rock_paper_scissors(),
            _ => games::menu(),
        }
    }
}

fn tool_menu(rest: &str) {
    if rest.is_empty() {
        tools::menu();
    } else {
        match rest {
            "calc" => tools::calculator_interactive(),
            "ascii" => tools::ascii_table(),
            _ => tools::menu(),
        }
    }
}

fn cmd_color(rest: &str) {
    let mut it = rest.split_whitespace();
    let fg = it.next().and_then(|s| s.parse::<u8>().ok());
    let bg = it.next().and_then(|s| s.parse::<u8>().ok());
    match (fg, bg) {
        (Some(f @ 0..=15), Some(b @ 0..=15)) => {
            vga::with_writer(|w| {
                w.set_color(
                    unsafe { core::mem::transmute::<u8, Color>(f) },
                    unsafe { core::mem::transmute::<u8, Color>(b) },
                );
            });
            println!("Color set to fg={} bg={}", f, b);
        }
        _ => println!("Usage: color <foreground 0-15> <background 0-15>"),
    }
}

fn reboot() {
    println!("Rebooting...");
    while keyboard::controller_busy() {}
    unsafe {
        crate::io::outb(0x64, 0xFE);
    }
    halt_loop();
}

// 保留 wait_key 引用避免未使用告警（setup/games 已用）
#[allow(dead_code)]
fn _wait_key() { wait_key(); }
