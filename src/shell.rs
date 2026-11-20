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
        "MLOS v1.0.5 - Type 'help' for commands.\n",
        "MLOS v1.0.5 - 输入 help 查看命令。\n"
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
        "version" => tprintln!("MLOS version 1.0.5 (x86_64)", "MLOS 版本 1.0.5 (x86_64)"),
        "about" => cmd_about(),
        "whoami" => println!("{}", current_user()),
        "uname" => println!("MLOS 1.0.5 mlos-host x86_64 GNU/Rust"),
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
        "usb" => cmd_usb(),
        "resolution" => cmd_resolution(),
        "update" => cmd_update(),
        "gui" => cmd_gui(rest),
        "perf" => cmd_perf(),
        "ls" | "files" => cmd_ls(),
        "cat" => cmd_cat(rest),
        "write" => cmd_write(rest),
        "rm" => cmd_rm(rest),
        "setup" => cmd_setup(cfg),
        "bootinstall" => cmd_bootinstall(),
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
    tprintln!("  perf               Performance monitor (CPU/mem/disk)", "  perf               性能监视器(CPU/内存/硬盘)");
    tprintln!("  usb                USB controllers & compatibility (1.0-3.2)", "  usb                USB 控制器与兼容性(1.0-3.2)");
    tprintln!("  resolution         Display resolution info (adaptive)", "  resolution         显示分辨率信息(自适应)");
    tprintln!("  gui on             Launch experimental GUI (asked to confirm)", "  gui on             启动实验性 GUI(需确认)");
    tprintln!("  ls                 List files on disk", "  ls                 列出硬盘文件");
    tprintln!("  cat <file>         Show file content", "  cat <文件>         查看文件内容");
    tprintln!("  write <file> <text> Write a file", "  write <文件> <内容> 写入文件");
    tprintln!("  rm <file>          Delete a file", "  rm <文件>          删除文件");
    tprintln!("  color <fg> <bg>    Set color (0-15)", "  color <前景> <背景> 设置颜色(0-15)");
    tprintln!("  banner             Redraw banner", "  banner             重绘横幅");
    tprintln!("  setup              Re-run setup wizard", "  setup              重新运行安装向导");
    tprintln!("  bootinstall        (Re)install disk bootloader & system files", "  bootinstall        (重新)安装硬盘引导程序与系统文件");
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
    tprintln!("  - Display : VGA text mode + adaptive resolution", "  - 显示    : VGA 文本模式 + 自适应分辨率");
    tprintln!("  - USB     : 1.0 / 1.1 / 2.0 / 3.0 / 3.1 / 3.2 host compatible", "  - USB     : 兼容 USB 1.0/1.1/2.0/3.0/3.1/3.2 主控");
    tprintln!("  - Input   : PS/2 keyboard", "  - 输入    : PS/2 键盘");
    tprintln!("  - Storage : ATA PIO hard disk (persistent config)", "  - 存储    : ATA PIO 硬盘(配置持久化)");
    tprintln!("  - Core    : no_std bare-metal kernel", "  - 内核    : no_std 裸机内核");
}

fn cmd_bootinstall() {
    println!("");
    vga::with_writer(|w| {
        w.set_color(Color::LightBlue, Color::Black);
        w.put_str(tr!("  === Disk Boot Installer ===\n", "  === 硬盘引导安装 ===\n"));
        w.set_color(Color::White, Color::Black);
    });
    tprintln!("  Writing MBR, stage-2 bootloader and kernel to disk...",
              "  正在写入 MBR、第二阶段引导程序与内核...");
    match crate::installer::install() {
        Ok(n) => {
            vga::with_writer(|w| {
                w.set_color(Color::LightGreen, Color::Black);
                let _ = w.write_fmt(format_args!("  SUCCESS: {} sectors written.\n", n));
                w.set_color(Color::White, Color::Black);
            });
            tprintln!("  The hard disk can now boot MLOS without the ISO.",
                      "  硬盘现在可以直接引导 MLOS，无需 ISO。");
        }
        Err(e) => {
            vga::with_writer(|w| w.set_color(Color::LightRed, Color::Black));
            tprintln!("  FAILED: {}", "  失败: {}", e);
            vga::with_writer(|w| w.set_color(Color::White, Color::Black));
        }
    }
    println!("");
}

fn cmd_usb() {
    println!("");
    vga::with_writer(|w| {
        w.set_color(Color::LightBlue, Color::Black);
        w.put_str(tr!("  === USB Compatibility (1.0 - 3.2) ===\n", "  === USB 兼容性 (1.0 - 3.2) ===\n"));
        w.set_color(Color::White, Color::Black);
    });
    tprintln!("  Support      : UHCI / OHCI (1.0, 1.1), EHCI (2.0), xHCI (3.0/3.1/3.2)",
              "  支持范围     : UHCI/OHCI (1.0, 1.1), EHCI (2.0), xHCI (3.0/3.1/3.2)");
    if !crate::usb::is_scanned() {
        tprintln!("  (scanning...)", "  (扫描中...)");
    }
    let n = crate::usb::scan();
    if n == 0 {
        tprintln!("  Controllers  : none detected on the PCI bus",
                  "  控制器       : PCI 总线上未检测到 USB 主控");
    } else {
        tprintln!("  Controllers  : {}", "  控制器       : {}", n);
        crate::usb::each(|c| {
            tprintln!("   - {} at {:X}:{:X}.{}  {}",
                      "   - {} 位于 {:X}:{:X}.{}  {}",
                      c.kind, c.bus, c.device, c.function, c.speed.label());
        });
    }
    println!("");
}

fn cmd_resolution() {
    println!("");
    vga::with_writer(|w| {
        w.set_color(Color::LightBlue, Color::Black);
        w.put_str(tr!("  === Display Resolution (adaptive) ===\n", "  === 显示分辨率 (自适应) ===\n"));
        w.set_color(Color::White, Color::Black);
    });
    let mut buf = alloc::string::String::new();
    crate::video::report(&mut buf);
    println!("{}", buf);
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
        let st = crate::installer::status_line();
        println!("{}", st);
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

/// Experimental GUI launcher. Disabled by default; `gui on` asks for
/// confirmation because the GUI is experimental and may misbehave.
fn cmd_gui(rest: &str) {
    if rest.trim() != "on" {
        println!("GUI is currently OFF (default).");
        println!("Usage: gui on");
        return;
    }
    println!("");
    vga::with_writer(|w| {
        w.set_color(Color::LightRed, Color::Black);
        w.put_str("  *** WARNING: EXPERIMENTAL FEATURE ***\n");
        w.set_color(Color::White, Color::Black);
    });
    println!("The GUI is experimental and is NOT guaranteed to work correctly.");
    println!("It may freeze, display artifacts, or behave unexpectedly.");
    print!("Do you really want to enable it? Type 'yes' to continue: ");
    let mut buf = [0u8; 16];
    let n = read_line(&mut buf, None);
    let ans = core::str::from_utf8(&buf[..n]).unwrap_or("").trim();
    if !ans.eq_ignore_ascii_case("yes") {
        println!("GUI launch cancelled. Staying in text shell.");
        return;
    }
    println!("Starting GUI...");
    crate::gui::run();
    println!("Type 'help' for commands.");
}

/// Performance monitor: CPU / memory / disk.
fn cmd_perf() {
    println!("");
    vga::with_writer(|w| {
        w.set_color(Color::LightBlue, Color::Black);
        w.put_str("  === Performance Monitor ===\n");
        w.set_color(Color::White, Color::Black);
    });

    // CPU
    let cpu = crate::perf::cpu_info();
    let vendor = core::str::from_utf8(&cpu.vendor[..12]).unwrap_or("unknown");
    vga::with_writer(|w| w.set_color(Color::LightCyan, Color::Black));
    tprintln!("  [CPU]", "  [CPU]");
    vga::with_writer(|w| w.set_color(Color::White, Color::Black));
    tprintln!("  Vendor        : {}", "  厂商          : {}", vendor);
    let mhz = cpu.tsc_hz / 1_000_000;
    if cpu.tsc_hz > 0 {
        tprintln!("  TSC frequency : ~{} MHz", "  TSC 频率      : ~{} MHz", mhz);
    } else {
        tprintln!("  TSC frequency : unavailable", "  TSC 频率      : 不可用");
    }
    tprintln!("  TSC cycles    : {}", "  TSC 周期      : {}", cpu.tsc_now);

    // Memory
    let mem = crate::perf::mem_info();
    vga::with_writer(|w| w.set_color(Color::LightCyan, Color::Black));
    tprintln!("  [MEMORY]", "  [内存]");
    vga::with_writer(|w| w.set_color(Color::White, Color::Black));
    tprintln!("  Total usable  : {} KiB ({} MiB)", "  可用总量      : {} KiB ({} MiB)", mem.total_kib, mem.total_kib / 1024);
    tprintln!("  Kernel used   : ~{} KiB", "  内核占用      : ~{} KiB", mem.kernel_kib);
    tprintln!("  Free (est.)   : {} KiB", "  剩余(估算)    : {} KiB", mem.free_kib);
    if mem.total_kib > 0 {
        let used_pct = (mem.kernel_kib * 100) / mem.total_kib.max(1);
        tprintln!("  Usage         : {}%", "  使用率        : {}%", used_pct.min(100));
    }

    // Disk
    let disk = crate::perf::disk_info();
    vga::with_writer(|w| w.set_color(Color::LightCyan, Color::Black));
    tprintln!("  [DISK]", "  [硬盘]");
    vga::with_writer(|w| w.set_color(Color::White, Color::Black));
    if !disk.present {
        tprintln!("  No hard disk detected.", "  未检测到硬盘。");
    } else {
        tprintln!("  Total         : {} sectors ({} KiB)", "  总容量        : {} 扇区 ({} KiB)", disk.total_sectors, disk.total_sectors / 2);
        tprintln!("  Used by MLOS  : {} sectors", "  已用          : {} 扇区", disk.used_sectors);
        let free = disk.total_sectors.saturating_sub(disk.used_sectors);
        tprintln!("  Free          : {} sectors ({} KiB)", "  剩余          : {} 扇区 ({} KiB)", free, free / 2);
    }
    println!("");
}

/// File browser: list files stored on the hard disk.
fn cmd_ls() {
    println!("");
    match crate::fs::list() {
        Some((entries, count)) => {
            if count == 0 {
                println!("(no files - use: write <name> <text>)");
                return;
            }
            vga::with_writer(|w| {
                w.set_color(Color::LightCyan, Color::Black);
                w.put_str("  NAME                              SIZE (bytes)\n");
                w.set_color(Color::White, Color::Black);
            });
            for e in entries.into_iter().flatten() {
                let name = e.name_str();
                tprintln!("  {:<32} {:>8}", "  {:<32} {:>8}", name, e.size);
            }
            tprintln!("  {} file(s)", "  共 {} 个文件", count);
        }
        None => println!("fs: cannot read directory sector."),
    }
    println!("");
}

/// Show the content of a file.
fn cmd_cat(rest: &str) {
    let name = rest.trim();
    if name.is_empty() {
        println!("Usage: cat <file>");
        return;
    }
    let mut buf = [0u8; crate::fs::MAX_FILE_SIZE];
    match crate::fs::read(name, &mut buf) {
        Some(n) => {
            println!("--- {} ({} bytes) ---", name, n);
            let text = core::str::from_utf8(&buf[..n]).unwrap_or("(binary data)");
            println!("{}", text);
            println!("--- end ---");
        }
        None => println!("cat: no such file: {}", name),
    }
}

/// Write (create or overwrite) a file with text content.
fn cmd_write(rest: &str) {
    let mut parts = rest.splitn(2, char::is_whitespace);
    let name = match parts.next() {
        Some(n) => n.trim(),
        None => "",
    };
    let text = parts.next().unwrap_or("").trim();
    if name.is_empty() || text.is_empty() {
        println!("Usage: write <file> <text>");
        return;
    }
    if crate::fs::write(name, text.as_bytes()) {
        println!("Wrote {} bytes to '{}'.", text.len(), name);
    } else {
        println!("write: failed (disk error, name too long, or no free slots).");
    }
}

/// Delete a file.
fn cmd_rm(rest: &str) {
    let name = rest.trim();
    if name.is_empty() {
        println!("Usage: rm <file>");
        return;
    }
    if crate::fs::delete(name) {
        println!("Deleted '{}'.", name);
    } else {
        println!("rm: no such file: {}", name);
    }
}
