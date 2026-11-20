//! 交互式命令行 Shell（MLOS）

use crate::games;
use crate::input::{read_line, wait_key};
use crate::keyboard::{self, Key, Keyboard};
use crate::storage::Config;
use crate::tools;
use crate::vga::{self, Color};
use crate::{halt_loop, println, print, tr, tprintln, TOTAL_MEMORY};
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
    println!("{}", tr!(
        "MLOS v1.0.1 - Type 'help' for commands, 'render gfx' for Chinese.\n",
        "MLOS v1.0.1 - 输入 help 查看命令，render text 切换英文。\n"
    ));
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
        "version" => tprintln!("MLOS version 1.0.1 (x86_64)", "MLOS 版本 1.0.1 (x86_64)"),
        "about" => cmd_about(),
        "render" => cmd_render(rest),
        "whoami" => println!("{}", current_user()),
        "uname" => println!("MLOS 1.0.1 mlos-host x86_64 GNU/Rust"),
        "date" => tprintln!("Build time: {}", "编译时间: {}", env!("BUILD_TIME")),
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
        "update" => cmd_update(),
        "setup" => cmd_setup(cfg),
        _ => println!("mlos: command not found: {}\nType 'help' for available commands.", cmd),
    }
}

fn cmd_help() {
    println!("");
    vga::with_writer(|w| {
        w.set_color(Color::LightGreen, Color::Black);
        w.put_str(tr!("  === MLOS Commands ===\n", "  === MLOS 命令列表 ===\n"));
        w.set_color(Color::White, Color::Black);
    });
    tprintln!("  help               Show this help", "  help               显示此帮助");
    tprintln!("  clear              Clear the screen", "  clear              清屏");
    tprintln!("  echo <text>        Print text", "  echo <文本>        输出文本");
    tprintln!("  version            Show version", "  version            显示版本");
    tprintln!("  about              About MLOS", "  about              关于系统");
    tprintln!("  whoami             Show current user", "  whoami             显示当前用户");
    tprintln!("  uname              System information", "  uname              显示系统信息");
    tprintln!("  date               Show build time", "  date               显示编译时间");
    tprintln!("  mem                Show memory", "  mem                显示内存");
    tprintln!("  disk               Show disk status", "  disk               显示硬盘状态");
    tprintln!("  update             Check for updates", "  update             检测系统更新");
    tprintln!("  color <fg> <bg>    Set color (0-15)", "  color <前景> <背景> 设置颜色(0-15)");
    tprintln!("  banner             Redraw banner", "  banner             重绘横幅");
    tprintln!("  render text/gfx    Switch rendering", "  render text/gfx    切换渲染方式");
    tprintln!("  setup              Re-run setup wizard", "  setup              重新运行安装向导");
    tprintln!("  reboot             Reboot system", "  reboot             重启系统");
    tprintln!("  halt               Halt system", "  halt               停机");
    println!("");
    tprintln!("  GAMES:  games | bomb | guess | rps", "  游戏:  games | bomb | guess | rps");
    tprintln!("  TOOLS:  tools | calc | base | prime | hash | rand | ascii",
              "  工具:  tools | calc | base | prime | hash | rand | ascii");
}

fn cmd_about() {
    tprintln!("MLOS - Minimal Light Operating System written in Rust",
              "MLOS - 用 Rust 编写的轻量操作系统");
    tprintln!("  - Author  : MEME-LMXDG", "  - 开发者  : MEME-LMXDG");
    tprintln!("  - Arch    : x86_64 (64-bit)", "  - 架构    : x86_64 (64位)");
    tprintln!("  - Boot    : GRUB 2 (Multiboot2)", "  - 引导    : GRUB 2 (Multiboot2)");
    tprintln!("  - Display : dual-mode (text / CJK gfx)", "  - 显示    : 双模式(文本/中文图形)");
    tprintln!("  - Input   : PS/2 keyboard", "  - 输入    : PS/2 键盘");
    tprintln!("  - Storage : ATA PIO hard disk (persistent config)", "  - 存储    : ATA PIO 硬盘(配置持久化)");
    tprintln!("  - Core    : no_std bare-metal kernel", "  - 内核    : no_std 裸机内核");
}

fn cmd_mem() {
    let kib = TOTAL_MEMORY.load(Ordering::Relaxed);
    if kib == 0 {
        tprintln!("Memory: unknown", "内存: 未知");
    } else {
        tprintln!("Usable memory: {} KiB ({} MiB)", "可用内存: {} KiB ({} MiB)", kib, kib / 1024);
    }
}

fn cmd_disk() {
    println!("");
    vga::with_writer(|w| {
        w.set_color(Color::LightBlue, Color::Black);
        w.put_str(tr!("  === Hard Disk Status ===\n", "  === 硬盘状态 ===\n"));
        w.set_color(Color::White, Color::Black);
    });
    if crate::ata::detect() {
        tprintln!("  Device      : Primary Master (ATA PIO)", "  设备        : 主盘 (ATA PIO)");
        tprintln!("  Config LBA  : 1", "  配置扇区    : 1");
        match crate::storage::load() {
            Some(cfg) => {
                tprintln!("  Config      : VALID (installed={})", "  配置        : 有效 (installed={})", cfg.installed);
                tprintln!("  Username    : {}", "  用户名      : {}", cfg.user_str());
            }
            None => tprintln!("  Config      : not found / invalid", "  配置        : 未找到或无效"),
        }
    } else {
        tprintln!("  No hard disk detected.", "  未检测到硬盘。");
    }
}

fn cmd_update() {
    use crate::update::{self, UpdateStatus};

    println!("");
    vga::with_writer(|w| {
        w.set_color(Color::LightBlue, Color::Black);
        w.put_str(tr!("  === Update Checker ===\n", "  === 系统更新检测 ===\n"));
        w.set_color(Color::White, Color::Black);
    });
    tprintln!("  Local version : {}", "  本地版本     : {}", update::LOCAL_VERSION);

    let info = update::check();
    match info.status {
        UpdateStatus::NoInfo => {
            tprintln!("  Remote        : {}", "  云端版本     : {}", tr!("no info", "未获取到"));
            tprintln!("  Status        : {}", "  检测结果     : {}", tr!(
                "Cannot read remote version from disk.",
                "无法从硬盘读取云端版本信息。"
            ));
            tprintln!("  {}", "  {}", tr!(
                "Run tools/check_update.sh on the host to fetch the latest version,",
                "请在主机运行 tools/check_update.sh 拉取最新版本信息，"
            ));
            tprintln!("  {}", "  {}", tr!(
                "then reboot MLOS and run 'update' again.",
                "然后重启 MLOS 再次运行 update。"
            ));
        }
        UpdateStatus::UpToDate => {
            tprintln!("  Remote        : {}", "  云端版本     : {}", info.remote_str());
            tprintln!("  Status        : {}", "  检测结果     : {}", tr!(
                "You are up to date.",
                "已是最新版本。"
            ));
        }
        UpdateStatus::NewAvailable => {
            vga::with_writer(|w| w.set_color(Color::LightRed, Color::Black));
            tprintln!("  New version {} available!", "  发现新版本 {} 可更新！", info.remote_str());
            vga::with_writer(|w| w.set_color(Color::White, Color::Black));
            tprintln!("  Download      : {}", "  下载地址     : {}", update::RELEASES_URL);
        }
    }
    println!("");
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

/// 自由切换两种渲染方式：text=英文文本，gfx=中文图形。
fn cmd_render(rest: &str) {
    match rest.trim() {
        "text" | "en" => {
            vga::set_mode(vga::RenderMode::Text);
            crate::print_banner();
            println!("Switched to English text mode.");
        }
        "gfx" | "cn" | "zh" | "中文" => {
            vga::set_mode(vga::RenderMode::Gfx);
            crate::print_banner();
            println!("已切换到中文图形模式。");
        }
        _ => println!("{}", tr!(
            "Usage: render text | render gfx",
            "用法: render text | render gfx"
        )),
    }
}
