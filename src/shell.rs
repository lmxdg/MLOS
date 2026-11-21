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
const HIST_CAP: usize = 24;

/// 当前登录的用户名副本
static mut CURRENT_USER: [u8; 32] = [0; 32];
/// 虚拟当前工作目录（MLOS 的文件系统是扁平的，目录为逻辑视图）
static mut CWD: [u8; 64] = [0; 64];
/// 命令历史环形缓冲
static mut HISTORY: [[u8; INPUT_CAP]; HIST_CAP] = [[0; INPUT_CAP]; HIST_CAP];
static mut HIST_COUNT: usize = 0;

pub fn prompt() {
    let user = current_user();
    let dir = cwd_display();
    vga::with_writer(|w| {
        w.set_color(Color::LightGreen, Color::Black);
        let _ = w.write_fmt(format_args!("{}", user));
        w.set_color(Color::DarkGray, Color::Black);
        w.put_str("@");
        w.set_color(Color::LightCyan, Color::Black);
        w.put_str("mlos");
        w.set_color(Color::DarkGray, Color::Black);
        w.put_str(":");
        w.set_color(Color::LightBlue, Color::Black);
        w.put_str(dir);
        w.set_color(Color::DarkGray, Color::Black);
        w.put_str("$ ");
        w.set_color(Color::White, Color::Black);
        w.update_cursor();
    });
}

unsafe fn set_current_user(cfg: &Config) {
    CURRENT_USER = cfg.username;
    CWD = [0; 64];
    let home = b"/home/";
    CWD[..home.len()].copy_from_slice(home);
    let mut n = home.len();
    for &b in cfg.username.iter() {
        if b == 0 { break; }
        if n < CWD.len() { CWD[n] = b; n += 1; }
    }
}

fn cwd_str() -> &'static str {
    unsafe {
        let len = CWD.iter().position(|&b| b == 0).unwrap_or(CWD.len());
        core::str::from_utf8(&CWD[..len]).unwrap_or("/")
    }
}

/// 提示符中的目录：把 /home/<user> 显示为 ~（Linux 习惯）
fn cwd_display() -> &'static str {
    let cwd = cwd_str();
    let user = current_user();
    let mut home_buf = [0u8; 48];
    let prefix = b"/home/";
    home_buf[..prefix.len()].copy_from_slice(prefix);
    let ulen = user.len().min(40);
    home_buf[prefix.len()..prefix.len() + ulen].copy_from_slice(&user.as_bytes()[..ulen]);
    let home_len = prefix.len() + ulen;
    if cwd.as_bytes() == &home_buf[..home_len] {
        "~"
    } else if let Some(rest) = cwd.strip_prefix(core::str::from_utf8(&home_buf[..home_len]).unwrap_or("/home")) {
        // 静态拼接不可行：仅支持精确的 home 目录显示 ~
        let _ = rest;
        cwd
    } else {
        cwd
    }
}

fn push_history(line: &str) {
    unsafe {
        let slot = HIST_COUNT % HIST_CAP;
        HISTORY[slot] = [0; INPUT_CAP];
        let n = line.len().min(INPUT_CAP);
        HISTORY[slot][..n].copy_from_slice(&line.as_bytes()[..n]);
        HIST_COUNT = HIST_COUNT.saturating_add(1);
    }
}

fn current_user() -> &'static str {
    unsafe {
        let len = CURRENT_USER.iter().position(|&b| b == 0).unwrap_or(32);
        core::str::from_utf8(&CURRENT_USER[..len]).unwrap_or("user")
    }
}

pub fn run(cfg: &Config) -> ! {
    unsafe { set_current_user(cfg); }
    mark_boot_time();

    loop {
        vga::with_writer(|w| w.clear());
        println!("{}", tr!(
            "MLOS v1.0.6 (x86_64) - Type 'help' for commands.\n",
            "MLOS v1.0.6 (x86_64) - 输入 help 查看命令。\n"
        ));
        prompt();

        let mut kb = Keyboard::new();
        let mut buf = [0u8; INPUT_CAP];
        let mut len = 0usize;
        let mut logout = false;

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
                            if !line.trim().is_empty() {
                                push_history(line);
                            }
                            if execute(line, cfg) {
                                logout = true;
                                break;
                            }
                            len = 0;
                            prompt();
                        }
                        _ => {}
                    }
                }
            }
            unsafe { core::arch::asm!("pause"); }
        }

        if logout {
            println!("{}", tr!("Logging out...", "正在注销..."));
            if !crate::setup::login(cfg) {
                crate::halt_loop();
            }
        }
    }
}

/// 执行命令。返回 true 表示注销退出（由调用方处理重新登录）。
fn execute(line: &str, cfg: &Config) -> bool {
    let line = line.trim();
    if line.is_empty() {
        return false;
    }
    let cmd_end = line.find(char::is_whitespace).unwrap_or(line.len());
    let cmd = &line[..cmd_end];
    let rest = line[cmd_end..].trim();

    match cmd {
        // 基础命令（类 Linux）
        "help" | "?" => cmd_help(),
        "man" | "info" => cmd_man(rest),
        "clear" | "cls" => vga::with_writer(|w| w.clear()),
        "echo" => println!("{}", rest),
        "version" => tprintln!("MLOS version 1.0.6 (x86_64)", "MLOS 版本 1.0.6 (x86_64)"),
        "lang" => cmd_lang(rest, cfg),
        "about" => cmd_about(),
        "whoami" => println!("{}", current_user()),
        "who" | "id" => cmd_who(),
        "hostname" => println!("mlos"),
        "uname" => cmd_uname(rest),
        "date" => tprintln!("Build time: {}", "编译时间: {}", env!("BUILD_TIME")),
        "uptime" => cmd_uptime(),
        "pwd" => cmd_pwd(),
        "cd" => cmd_cd(rest),
        "history" => cmd_history(),
        "exit" | "logout" => return true,
        "mem" | "free" => cmd_mem(),
        "df" => cmd_df(),
        "ps" => cmd_ps(),
        "top" => cmd_perf(),
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
        "mkdir" | "touch" => cmd_touch(rest),
        "grep" => cmd_grep(rest),
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
        "ls" | "files" => cmd_ls(rest),
        "cat" => cmd_cat(rest),
        "write" => cmd_write(rest),
        "rm" => cmd_rm(rest),
        "setup" => cmd_setup(cfg),
        "bootinstall" => cmd_bootinstall(),
        _ => {
            println!("mlos: command not found: {}", cmd);
            tprintln!("Type 'help' for available commands.", "输入 help 查看可用命令。");
        }
    }
    false
}

fn cmd_help() {
    println!("");
    vga::with_writer(|w| {
        w.set_color(Color::LightGreen, Color::Black);
        w.put_str(tr!("  === MLOS Shell - Commands ===\n", "  === MLOS Shell - 命令列表 ===\n"));
        w.set_color(Color::White, Color::Black);
    });
    tprintln!("  [general]", "  [常规]");
    tprintln!("    help | man <cmd>   help / command manual", "    help | man <命令>  帮助/命令手册");
    tprintln!("    clear              clear the screen", "    clear              清屏");
    tprintln!("    echo <text>        print text", "    echo <文本>        输出文本");
    tprintln!("    history            show command history", "    history            显示命令历史");
    tprintln!("    exit | logout      log out and return to login", "    exit | logout      注销并返回登录界面");
    tprintln!("    reboot | halt      reboot / power off", "    reboot | halt      重启/停机");
    tprintln!("  [system]", "  [系统]");
    tprintln!("    version | uname    version and system info", "    version | uname    版本与系统信息");
    tprintln!("    date | uptime      build time / running time", "    date | uptime      编译时间/运行时间");
    tprintln!("    whoami | who | id  current user", "    whoami | who | id  当前用户");
    tprintln!("    hostname           print hostname", "    hostname           显示主机名");
    tprintln!("    mem | free | df | ps | top    resources", "    mem | free | df | ps | top  资源信息");
    tprintln!("    perf | disk | usb | resolution", "    perf | disk | usb | resolution  性能/磁盘/USB/分辨率");
    tprintln!("    lang en|zh         switch language (zh = preview)", "    lang en|zh         切换语言(zh 为预览版)");
    tprintln!("    color <fg> <bg>    set terminal colors (0-15)", "    color <前景> <背景> 设置终端颜色(0-15)");
    tprintln!("    banner             redraw the banner", "    banner             重绘横幅");
    tprintln!("  [files]", "  [文件]");
    tprintln!("    pwd | cd <path>    logical working directory", "    pwd | cd <路径>    逻辑工作目录");
    tprintln!("    ls [-l]            list files", "    ls [-l]            列出文件");
    tprintln!("    cat [-n] <file>    show file content", "    cat [-n] <文件>    查看文件内容");
    tprintln!("    write <f> <text>   create/overwrite a file", "    write <文件> <内容> 创建/覆盖文件");
    tprintln!("    touch <file>       create an empty file", "    touch <文件>       创建空文件");
    tprintln!("    rm <file>          remove a file", "    rm <文件>          删除文件");
    tprintln!("    grep <pat> <file>  search text in a file", "    grep <模式> <文件> 在文件中搜索文本");
    tprintln!("  [setup]", "  [维护]");
    tprintln!("    gui [on]           launch the mouse-driven desktop", "    gui [on]           启动鼠标驱动的桌面");
    tprintln!("    update             check for updates", "    update             检测系统更新");
    tprintln!("    setup              re-run the setup wizard", "    setup              重新运行安装向导");
    tprintln!("    bootinstall        install disk bootloader", "    bootinstall        安装硬盘引导程序");
    tprintln!("  [games]  games | bomb | guess | rps", "  [游戏]  games | bomb | guess | rps");
    tprintln!("  [tools]  tools | calc | base | prime | hash | rand | ascii",
              "  [工具]  tools | calc | base | prime | hash | rand | ascii");
    println!("");
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

fn set_cwd(s: &str) {
    unsafe {
        CWD = [0; 64];
        let n = s.len().min(63);
        CWD[..n].copy_from_slice(&s.as_bytes()[..n]);
    }
}

fn set_home_cwd() {
    let user = current_user();
    let ulen = user.len().min(40);
    let mut home = [0u8; 48];
    home[..6].copy_from_slice(b"/home/");
    home[6..6 + ulen].copy_from_slice(&user.as_bytes()[..ulen]);
    set_cwd(core::str::from_utf8(&home[..6 + ulen]).unwrap_or("/"));
}

fn cmd_pwd() {
    println!("{}", cwd_str());
}

fn cmd_cd(rest: &str) {
    let t = rest.trim();
    let user = current_user();
    let ulen = user.len().min(40);
    let mut home_buf = [0u8; 48];
    home_buf[..6].copy_from_slice(b"/home/");
    home_buf[6..6 + ulen].copy_from_slice(&user.as_bytes()[..ulen]);
    let home = core::str::from_utf8(&home_buf[..6 + ulen]).unwrap_or("/home");

    if t.is_empty() || t == "~" || t == home {
        set_home_cwd();
    } else if t == "/" {
        set_cwd("/");
    } else if t == "." {
        // 当前目录不变
    } else if t == ".." {
        let cwd = cwd_str();
        if cwd == home {
            set_cwd("/");
        } else if let Some(p) = cwd.rfind('/') {
            set_cwd(if p == 0 { "/" } else { &cwd[..p] });
        } else {
            set_cwd("/");
        }
    } else if t.starts_with('/') {
        set_cwd(t);
    } else {
        let cwd = cwd_str();
        let mut path = [0u8; 128];
        let base = cwd.as_bytes();
        let sep = if cwd.ends_with('/') { 0 } else { 1 };
        let n = base.len().min(120usize.saturating_sub(sep + t.len()));
        path[..n].copy_from_slice(&base[..n]);
        if sep == 1 && n + 1 < path.len() {
            path[n] = b'/';
        }
        let off = n + sep;
        let tn = t.len().min(path.len().saturating_sub(off));
        path[off..off + tn].copy_from_slice(&t.as_bytes()[..tn]);
        set_cwd(core::str::from_utf8(&path[..off + tn]).unwrap_or("/"));
    }
}

fn cmd_history() {
    unsafe {
        let n = HIST_COUNT.min(HIST_CAP);
        let start = HIST_COUNT.saturating_sub(n);
        for i in 0..n {
            let idx = (start + i) % HIST_CAP;
            let len = HISTORY[idx].iter().position(|&b| b == 0).unwrap_or(INPUT_CAP);
            let line = core::str::from_utf8(&HISTORY[idx][..len]).unwrap_or("");
            println!("{:>4}  {}", i + 1, line);
        }
    }
}

fn cmd_who() {
    let user = current_user();
    tprintln!("{} tty1        {} (local)", "{} tty1        {} (本地)", user, env!("BUILD_TIME"));
    tprintln!("  (MLOS single-user console)", "  (MLOS 单用户控制台)");
}

fn cmd_uname(rest: &str) {
    let (mut all, mut rel, mut mach) = (false, false, false);
    for arg in rest.split_whitespace() {
        match arg {
            "-a" | "--all" => all = true,
            "-r" => rel = true,
            "-m" => mach = true,
            _ => {}
        }
    }
    if all {
        println!("MLOS 1.0.6 mlos-host x86_64 GNU/Rust");
        tprintln!("Build: {}", "编译: {}", env!("BUILD_TIME"));
    } else if rel {
        println!("1.0.6");
    } else if mach {
        println!("x86_64");
    } else {
        println!("MLOS");
    }
}

static mut BOOT_TSC: u64 = 0;

/// 在 shell 首次启动时记录开机 TSC，供 uptime 使用
pub fn mark_boot_time() {
    unsafe { BOOT_TSC = crate::io::rdtsc(); }
}

fn cmd_uptime() {
    let cpu = crate::perf::cpu_info();
    let hz = if cpu.tsc_hz > 0 { cpu.tsc_hz } else { 2_000_000_000 };
    let secs = unsafe { cpu.tsc_now.wrapping_sub(BOOT_TSC) } / hz;
    let (h, m, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    tprintln!(" up {:>2}:{:02}:{:02},  1 user,  load average: 0.00, 0.00, 0.00",
              " 已运行 {:>2}:{:02}:{:02},  1 位用户,  平均负载: 0.00, 0.00, 0.00", h, m, s);
}

fn cmd_df() {
    let disk = crate::perf::disk_info();
    tprintln!("Filesystem     1K-blocks     Used Available Use%", "文件系统        1K-块       已用   可用    使用率");
    if disk.present {
        let total = disk.total_sectors / 2;
        let used = disk.used_sectors / 2;
        let avail = total.saturating_sub(used);
        let pct = if total > 0 { used * 100 / total } else { 0 };
        println!("/dev/ata0      {:>10} {:>7} {:>10} {:>3}%", total, used, avail, pct.min(100));
    } else {
        println!("/dev/ata0      (no disk)");
    }
}

fn cmd_ps() {
    tprintln!("  PID TTY          TIME CMD", "  PID 终端         时间 命令");
    println!("    1 ?        00:00:00 kernel");
    println!("    2 tty1     00:00:00 shell");
    println!("    3 tty1     00:00:00 init");
}

fn cmd_touch(rest: &str) {
    let name = rest.split_whitespace().next().unwrap_or("").trim();
    if name.is_empty() {
        tprintln!("Usage: touch <file> | mkdir <dir>", "用法: touch <文件> | mkdir <目录>");
        return;
    }
    if crate::fs::write(name, b" ") {
        tprintln!("Created '{}'.", "已创建 '{}'。", name);
    } else {
        tprintln!("Failed to create '{}'.", "创建 '{}' 失败。", name);
    }
}

fn cmd_grep(rest: &str) {
    let mut parts = rest.splitn(2, char::is_whitespace);
    let pat = parts.next().unwrap_or("").trim();
    let name = parts.next().unwrap_or("").trim();
    if pat.is_empty() || name.is_empty() {
        tprintln!("Usage: grep <pattern> <file>", "用法: grep <模式> <文件>");
        return;
    }
    let mut buf = [0u8; crate::fs::MAX_FILE_SIZE];
    match crate::fs::read(name, &mut buf) {
        Some(n) => {
            let text = core::str::from_utf8(&buf[..n]).unwrap_or("");
            let mut found = 0usize;
            for (i, line) in text.lines().enumerate() {
                if line.contains(pat) {
                    println!("{}:{}:{}", name, i + 1, line);
                    found += 1;
                }
            }
            if found == 0 {
                tprintln!("grep: no match for '{}'", "grep: 未找到匹配 '{}'", pat);
            }
        }
        None => tprintln!("grep: no such file: {}", "grep: 文件不存在: {}", name),
    }
}

/// 命令手册（简化版 man）
fn cmd_man(rest: &str) {
    let name = rest.split_whitespace().next().unwrap_or("").trim();
    if name.is_empty() || name == "-h" || name == "--help" {
        tprintln!("Usage: man <command>", "用法: man <命令>");
        return;
    }
    let (en, zh): (&str, &str) = match name {
        "ls" => (
            "NAME\n  ls - list directory contents\nSYNOPSIS\n  ls [-l] [-a]\nDESCRIPTION\n  List files stored on the MLOS disk. -l shows sizes.",
            "名称\n  ls - 列出目录内容\n用法\n  ls [-l] [-a]\n说明\n  列出 MLOS 硬盘上的文件，-l 显示大小。",
        ),
        "cd" => (
            "NAME\n  cd - change the logical working directory\nSYNOPSIS\n  cd [path | ~ | .. | /]\nDESCRIPTION\n  Changes the prompt path. MLOS uses a logical directory view.",
            "名称\n  cd - 切换逻辑工作目录\n用法\n  cd [路径 | ~ | .. | /]\n说明\n  改变提示符中的路径，MLOS 使用逻辑目录视图。",
        ),
        "cat" => (
            "NAME\n  cat - print file content\nSYNOPSIS\n  cat [-n] <file>\nDESCRIPTION\n  Prints the content of a file on the MLOS disk.",
            "名称\n  cat - 显示文件内容\n用法\n  cat [-n] <文件>\n说明\n  显示 MLOS 硬盘上文件的内容。",
        ),
        "lang" => (
            "NAME\n  lang - switch interface language\nSYNOPSIS\n  lang en | lang zh\nDESCRIPTION\n  zh enables the Chinese preview font; en restores English.",
            "名称\n  lang - 切换界面语言\n用法\n  lang en | lang zh\n说明\n  zh 启用中文预览字库，en 恢复英文。",
        ),
        "gui" => (
            "NAME\n  gui - launch the desktop\nSYNOPSIS\n  gui [on]\nDESCRIPTION\n  Starts the mouse-driven text desktop. Press Esc or click Exit.",
            "名称\n  gui - 启动桌面\n用法\n  gui [on]\n说明\n  启动支持鼠标的文本桌面，按 Esc 或点击退出。",
        ),
        _ => (
            "No manual entry. Try: help",
            "没有该命令的手册，请输入 help 查看命令列表。",
        ),
    };
    println!("{}", tr!(en, zh));
}

/// 切换界面语言（中文为预览版），并把偏好写入硬盘
fn cmd_lang(rest: &str, _cfg: &Config) {
    let target = match rest.trim() {
        "zh" | "cn" | "chinese" => Some(true),
        "en" | "english" => Some(false),
        _ => None,
    };
    match target {
        Some(zh) => {
            crate::vga::set_lang(zh);
            if let Some(mut saved) = crate::storage::load() {
                saved.lang_zh = zh;
                crate::storage::save(&saved);
            }
            if zh {
                tprintln!("Chinese (preview) enabled and saved.", "已启用中文（预览版）并保存偏好。");
            } else {
                tprintln!("English enabled and saved.", "已切换为英文并保存偏好。");
            }
        }
        None => {
            tprintln!("Usage: lang en | lang zh", "用法: lang en | lang zh");
            tprintln!("  zh = Chinese (preview), en = English", "  zh = 中文（预览版）, en = 英文");
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

/// Launch the mouse-driven desktop (no experimental warning).
fn cmd_gui(rest: &str) {
    if matches!(rest.trim(), "off" | "false" | "0") {
        tprintln!("Desktop is not running.", "桌面未运行。");
        return;
    }
    tprintln!("Starting desktop (mouse + keyboard)...", "正在启动桌面（鼠标 + 键盘）...");
    crate::gui::run();
    tprintln!("Back to shell. Type 'help' for commands.", "已返回命令行，输入 help 查看命令。");
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
fn cmd_ls(rest: &str) {
    let long = rest.split_whitespace().any(|a| a == "-l" || a == "--long");
    let all = rest.split_whitespace().any(|a| a == "-a" || a == "--all");
    println!("");
    if all {
        println!(".");
        println!("..");
    }
    match crate::fs::list() {
        Some((entries, count)) => {
            if count == 0 {
                println!("(no files - use: write <name> <text>)");
                return;
            }
            if long {
                tprintln!("  total {}", "  总计 {}", count);
            } else {
                vga::with_writer(|w| {
                    w.set_color(Color::LightCyan, Color::Black);
                    w.put_str("  NAME                              SIZE (bytes)\n");
                    w.set_color(Color::White, Color::Black);
                });
            }
            for e in entries.into_iter().flatten() {
                let name = e.name_str();
                if long {
                    println!("  -rw-r--r--  {:>8}  {}", e.size, name);
                } else {
                    println!("  {:<32} {:>8}", name, e.size);
                }
            }
            tprintln!("  {} file(s)", "  共 {} 个文件", count);
        }
        None => println!("fs: cannot read directory sector."),
    }
    println!("");
}

/// Show the content of a file (with optional -n line numbers).
fn cmd_cat(rest: &str) {
    let (mut numbered, mut name) = (false, "");
    for p in rest.split_whitespace() {
        if p == "-n" {
            numbered = true;
        } else {
            name = p;
            break;
        }
    }
    if name.is_empty() {
        tprintln!("Usage: cat [-n] <file>", "用法: cat [-n] <文件>");
        return;
    }
    let mut buf = [0u8; crate::fs::MAX_FILE_SIZE];
    match crate::fs::read(name, &mut buf) {
        Some(n) => {
            println!("--- {} ({} bytes) ---", name, n);
            let text = core::str::from_utf8(&buf[..n]).unwrap_or("(binary data)");
            if numbered {
                for (i, line) in text.lines().enumerate() {
                    println!("{:>6}\t{}", i + 1, line);
                }
            } else {
                println!("{}", text);
            }
            println!("--- end ---");
        }
        None => tprintln!("cat: no such file: {}", "cat: 文件不存在: {}", name),
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
