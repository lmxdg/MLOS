//! 交互式命令行 Shell（MLOS）

use crate::games;
use crate::input::{read_line, wait_key};
use crate::keyboard::{self, Key, Keyboard};
use crate::storage::Config;
use crate::tools;
use crate::vga::{self, Color};
use crate::{halt_loop, println, print, tr, tprintln, tprint, TOTAL_MEMORY};
use crate::rtc;
use alloc::format;
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

pub fn prompt() -> (usize, usize) {
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
    vga::cursor_pos()
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

static mut CWD_DISPLAY: [u8; 96] = [0; 96];
static mut OLDPWD: [u8; 64] = [0; 64];

/// 提示符中的目录：把 /home/<user> 及其子目录显示为 ~/...
fn cwd_display() -> &'static str {
    let cwd = cwd_str();
    let user = current_user();
    let mut home_buf = [0u8; 48];
    let prefix = b"/home/";
    home_buf[..prefix.len()].copy_from_slice(prefix);
    let ulen = user.len().min(40);
    home_buf[prefix.len()..prefix.len() + ulen].copy_from_slice(&user.as_bytes()[..ulen]);
    let home_len = prefix.len() + ulen;
    let home = core::str::from_utf8(&home_buf[..home_len]).unwrap_or("/home");
    if cwd == home {
        return "~";
    }
    if let Some(rest) = cwd.strip_prefix(home) {
        let mut disp = [0u8; 96];
        disp[0] = b'~';
        let n = rest.len().min(94);
        disp[1..1 + n].copy_from_slice(&rest.as_bytes()[..n]);
        unsafe {
            CWD_DISPLAY = disp;
            let len = CWD_DISPLAY.iter().position(|&b| b == 0).unwrap_or(96);
            return core::str::from_utf8(&CWD_DISPLAY[..len]).unwrap_or("~");
        }
    }
    cwd
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

pub fn current_user() -> &'static str {
    unsafe {
        let len = CURRENT_USER.iter().position(|&b| b == 0).unwrap_or(32);
        core::str::from_utf8(&CURRENT_USER[..len]).unwrap_or("user")
    }
}

const COMMANDS: &[&str] = &[
    "about", "base", "banner", "bootinstall", "calc", "cat", "cd", "clear",
    "color", "cp", "date", "demo", "df", "disk", "dmesg", "echo", "env",
    "exit", "find", "free", "games", "grep", "gui", "halt", "head", "help",
    "history", "hostname", "hw", "id", "lang", "logout", "ls", "lscpu",
    "lspci", "lsusb", "man", "mem", "mkdir", "mv", "passwd", "perf", "prime",
    "ps", "pwd", "rand", "reboot", "resolution", "rm", "rps", "setup",
    "shutdown", "stat", "su", "tail", "tools", "top", "touch", "tree",
    "uname", "update", "uptime", "usb", "version", "wc", "who", "whoami",
    "write", "ll", "la",
];

fn clear_input_area(row: usize, col: usize) {
    let (cols, rows) = vga::size();
    vga::with_writer(|w| {
        for r in row..rows {
            w.set_pos(r, if r == row { col } else { 0 });
            for _ in (if r == row { col } else { 0 })..cols {
                w.put_byte(b' ');
            }
        }
        w.set_pos(row, col);
        w.set_color(Color::White, Color::Black);
    });
}

fn show_input(row: usize, col: usize, data: &[u8]) {
    clear_input_area(row, col);
    vga::with_writer(|w| {
        for &b in data { w.put_byte(b); }
    });
}

fn history_entry(idx: usize) -> (&'static [u8], usize) {
    unsafe {
        let len = HISTORY[idx].iter().position(|&b| b == 0).unwrap_or(INPUT_CAP);
        (&HISTORY[idx][..len], len)
    }
}

fn complete(buf: &mut [u8], len: &mut usize, row: usize, col: usize) -> bool {
    let line = core::str::from_utf8(&buf[..*len]).unwrap_or("");
    if line.contains(' ') {
        // 文件名补全（最后一个参数）
        let mut start = 0usize;
        for (i, ch) in line.char_indices() {
            if ch == ' ' { start = i + 1; }
        }
        let prefix = &line[start..];
        let mut found = [[0u8; 30]; 12];
        let mut flen = [0usize; 12];
        let mut n = 0usize;
        if let Some((entries, _)) = fs_list() {
            for e in entries.iter().flatten() {
                if e.name_str().starts_with(prefix) {
                    if n < 12 {
                        found[n] = e.name;
                        flen[n] = e.name_str().len();
                        n += 1;
                    }
                }
            }
        }
        if n == 1 {
            let name = core::str::from_utf8(&found[0][..flen[0]]).unwrap_or("");
            let add = &name[prefix.len()..];
            for b in add.bytes() {
                if *len < buf.len() - 1 { buf[*len] = b; *len += 1; }
            }
            show_input(row, col, &buf[..*len]);
            return true;
        }
        if n > 1 {
            vga::with_writer(|w| w.put_byte(b'\n'));
            for i in 0..n {
                println!("{}", core::str::from_utf8(&found[i][..flen[i]]).unwrap_or(""));
            }
            let start = prompt();
            show_input(start.0, start.1, &buf[..*len]);
        }
        return false;
    }
    let mut matches: [&str; 16] = [""; 16];
    let mut n = 0usize;
    for c in COMMANDS {
        if c.starts_with(line) {
            if n < 16 { matches[n] = c; n += 1; }
        }
    }
    if n == 1 {
        let add = &matches[0][line.len()..];
        for b in add.bytes() {
            if *len < buf.len() - 1 { buf[*len] = b; *len += 1; }
        }
        if *len < buf.len() - 1 { buf[*len] = b' '; *len += 1; }
        show_input(row, col, &buf[..*len]);
        true
    } else if n > 1 {
        vga::with_writer(|w| w.put_byte(b'\n'));
        let mut linebuf = alloc::string::String::new();
        for i in 0..n {
            if i > 0 { linebuf.push_str("  "); }
            linebuf.push_str(matches[i]);
        }
        println!("{}", linebuf);
        let start = prompt();
        show_input(start.0, start.1, &buf[..*len]);
        false
    } else {
        false
    }
}

fn fs_list() -> Option<([Option<crate::fs::FileEntry>; 12], usize)> {
    crate::fs::list()
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
        let (mut prow, mut pcol) = prompt();

        let mut kb = Keyboard::new();
        let mut buf = [0u8; INPUT_CAP];
        let mut len = 0usize;
        let mut draft = [0u8; INPUT_CAP];
        let mut draft_len = 0usize;
        let mut hist_cur = usize::MAX;

        loop {
            let sc = match keyboard::read_scancode() {
                Some(s) => s,
                None => { unsafe { core::arch::asm!("pause"); } continue; }
            };
            let key = match kb.handle_scancode(sc) {
                Some(k) => k,
                None => continue,
            };
            match key {
                Key::Char(ch) if ch.is_ascii() => {
                    if len < INPUT_CAP - 1 {
                        buf[len] = ch as u8;
                        len += 1;
                        vga::with_writer(|w| w.put_byte(ch as u8));
                    }
                    hist_cur = usize::MAX;
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
                        break;
                    }
                    len = 0;
                    hist_cur = usize::MAX;
                    draft_len = 0;
                    let p = prompt();
                    prow = p.0; pcol = p.1;
                }
                Key::Up => {
                    unsafe {
                        if HIST_COUNT > 0 {
                            let visible = HIST_COUNT.min(HIST_CAP);
                            let base = HIST_COUNT.saturating_sub(visible);
                            if hist_cur == usize::MAX {
                                draft[..len].copy_from_slice(&buf[..len]);
                                draft_len = len;
                                hist_cur = visible - 1;
                            } else if hist_cur > 0 {
                                hist_cur -= 1;
                            }
                            let (data, n) = history_entry((base + hist_cur) % HIST_CAP);
                            buf[..n].copy_from_slice(data);
                            len = n;
                            show_input(prow, pcol, &buf[..len]);
                        }
                    }
                }
                Key::Down => {
                    unsafe {
                        if hist_cur != usize::MAX {
                            let visible = HIST_COUNT.min(HIST_CAP);
                            let base = HIST_COUNT.saturating_sub(visible);
                            if hist_cur + 1 >= visible {
                                hist_cur = usize::MAX;
                                buf[..draft_len].copy_from_slice(&draft[..draft_len]);
                                len = draft_len;
                            } else {
                                hist_cur += 1;
                                let (data, n) = history_entry((base + hist_cur) % HIST_CAP);
                                buf[..n].copy_from_slice(data);
                                len = n;
                            }
                            show_input(prow, pcol, &buf[..len]);
                        }
                    }
                }
                Key::CtrlC => {
                    vga::with_writer(|w| { w.put_str("^C\n"); });
                    len = 0;
                    hist_cur = usize::MAX;
                    let p = prompt();
                    prow = p.0; pcol = p.1;
                }
                Key::CtrlL => {
                    vga::with_writer(|w| w.clear());
                    println!("{}", tr!(
                        "MLOS v1.0.6 (x86_64) - Type 'help' for commands.\n",
                        "MLOS v1.0.6 (x86_64) - 输入 help 查看命令。\n"
                    ));
                    let p = prompt();
                    prow = p.0; pcol = p.1;
                    show_input(prow, pcol, &buf[..len]);
                }
                Key::CtrlU => {
                    clear_input_area(prow, pcol);
                    len = 0;
                    hist_cur = usize::MAX;
                }
                Key::Tab => {
                    complete(&mut buf, &mut len, prow, pcol);
                }
                _ => {}
            }
        }

        println!("{}", tr!("Logging out...", "正在注销..."));
        let latest = crate::storage::load().unwrap_or(*cfg);
        if !crate::setup::login(&latest) {
            crate::halt_loop();
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
        "echo" => return cmd_echo(rest),
        "version" => tprintln!("MLOS version 1.0.6 (x86_64)", "MLOS 版本 1.0.6 (x86_64)"),
        "lang" => cmd_lang(rest, cfg),
        "about" => cmd_about(),
        "whoami" => println!("{}", current_user()),
        "who" | "id" => cmd_who(),
        "hostname" => println!("mlos"),
        "uname" => cmd_uname(rest),
        "date" => cmd_date(),
        "uptime" => cmd_uptime(),
        "pwd" => cmd_pwd(),
        "cd" => cmd_cd(rest),
        "history" => cmd_history(),
        "passwd" => cmd_passwd(),
        "env" => cmd_env(),
        "hw" => cmd_hw(),
        "lscpu" => cmd_lscpu(),
        "lspci" => cmd_lspci(),
        "lsusb" => cmd_usb(),
        "dmesg" => cmd_dmesg(),
        "demo" => cmd_demo(),
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
        "head" => cmd_head(rest),
        "tail" => cmd_tail(rest),
        "wc" => cmd_wc(rest),
        "cp" => cmd_cp(rest),
        "mv" => cmd_mv(rest),
        "find" => cmd_find(rest),
        "stat" => cmd_stat(rest),
        "tree" => cmd_tree(),
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
        "ll" => cmd_ls(&format!("-l {}", rest)),
        "la" => cmd_ls(&format!("-la {}", rest)),
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
    tprintln!("    date | uptime      RTC time / running time", "    date | uptime      实时时钟/运行时间");
    tprintln!("    env                environment variables", "    env                环境变量");
    tprintln!("    lscpu | lspci      CPU / PCI information", "    lscpu | lspci      CPU / PCI 信息");
    tprintln!("    dmesg | hw         boot log / hardware check", "    dmesg | hw         启动日志/硬件检测");
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
    tprintln!("    head/tail [-n N]   first/last lines", "    head/tail [-n N]   前/后若干行");
    tprintln!("    wc [-lwc] <file>   line/word/byte count", "    wc [-lwc] <文件>   行/词/字节统计");
    tprintln!("    cp | mv | find     copy / move / find files", "    cp | mv | find     复制/移动/查找文件");
    tprintln!("    stat <file>        file details", "    stat <文件>        文件详细信息");
    tprintln!("    tree               flat file tree", "    tree               文件树");
    tprintln!("    echo <text> > <f>  write text to a file", "    echo <文本> > <文件> 把文本写入文件");
    tprintln!("    demo               create a Chinese sample file", "    demo               创建中文示例文件");
    tprintln!("  [setup]", "  [维护]");
    tprintln!("    gui [on]           launch the mouse-driven desktop", "    gui [on]           启动鼠标驱动的桌面");
    tprintln!("    update             check for updates", "    update             检测系统更新");
    tprintln!("    setup              re-run the setup wizard", "    setup              重新运行安装向导");
    tprintln!("    passwd             change your password", "    passwd             修改密码");
    tprintln!("    bootinstall        install disk bootloader", "    bootinstall        安装硬盘引导程序");
    tprintln!("  [keys]   Up/Down history, Tab complete, Ctrl+C cancel, Ctrl+L clear, Ctrl+U kill line",
              "  [按键]   Up/Down 历史，Tab 补全，Ctrl+C 取消，Ctrl+L 清屏，Ctrl+U 删除整行");
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

fn normalize_path(input: &str) -> [u8; 96] {
    let mut joined = [0u8; 192];
    let mut jn = 0usize;
    let cwd = cwd_str();
    if !input.starts_with('/') {
        let n = cwd.len().min(120);
        joined[..n].copy_from_slice(&cwd.as_bytes()[..n]);
        jn = n;
        if jn == 0 || joined[jn - 1] != b'/' {
            joined[jn] = b'/';
            jn += 1;
        }
    }
    let n = input.len().min(190 - jn);
    joined[jn..jn + n].copy_from_slice(&input.as_bytes()[..n]);
    jn += n;

    let mut comps = [[0u8; 32]; 16];
    let mut clen = [0usize; 16];
    let mut nc = 0usize;
    for part in core::str::from_utf8(&joined[..jn]).unwrap_or("/").split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            if nc > 0 { nc -= 1; }
            continue;
        }
        if nc < 16 {
            let n = part.len().min(31);
            comps[nc][..n].copy_from_slice(part.as_bytes());
            clen[nc] = n;
            nc += 1;
        }
    }
    let mut out = [0u8; 96];
    let mut on = 0usize;
    out[on] = b'/';
    on += 1;
    for i in 0..nc {
        if on > 1 { out[on] = b'/'; on += 1; }
        let n = clen[i].min(94 - on);
        out[on..on + n].copy_from_slice(&comps[i][..n]);
        on += n;
    }
    out
}

fn set_oldpwd() {
    unsafe {
        OLDPWD = [0; 64];
        let cur = cwd_str();
        let n = cur.len().min(63);
        OLDPWD[..n].copy_from_slice(&cur.as_bytes()[..n]);
    }
}

fn cmd_cd(rest: &str) {
    let t = rest.trim();
    let old = cwd_str();
    if t == "-" {
        unsafe {
            let len = OLDPWD.iter().position(|&b| b == 0).unwrap_or(64);
            if len == 0 {
                tprintln!("cd: OLDPWD not set", "cd: 未设置 OLDPWD");
                return;
            }
            let prev = core::str::from_utf8(&OLDPWD[..len]).unwrap_or("/");
            let mut tmp = [0u8; 64];
            let n = old.len().min(63);
            tmp[..n].copy_from_slice(old.as_bytes());
            set_cwd(prev);
            OLDPWD = tmp;
            println!("{}", cwd_str());
        }
        return;
    }
    if t.is_empty() || t == "~" {
        set_oldpwd();
        set_home_cwd();
    } else if t == "~/" || t.starts_with("~/") {
        set_oldpwd();
        let target = normalize_path(&t[1..]);
        let len = target.iter().position(|&b| b == 0).unwrap_or(95);
        set_cwd(core::str::from_utf8(&target[..len]).unwrap_or("/"));
    } else {
        set_oldpwd();
        let target = normalize_path(t);
        let len = target.iter().position(|&b| b == 0).unwrap_or(95);
        set_cwd(core::str::from_utf8(&target[..len]).unwrap_or("/"));
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
    let long = rest.split_whitespace().any(|a| a == "-l" || a == "--long" || a == "-la" || a == "-al");
    let all = rest.split_whitespace().any(|a| a == "-a" || a == "--all" || a == "-la" || a == "-al");
    println!("");
    if all {
        println!(".");
        println!("..");
    }
    match crate::fs::list() {
        Some((entries, count)) => {
            if count == 0 {
                tprintln!("(no files - use: write <name> <text>)", "（无文件，可用 write <名称> <内容> 创建）");
                return;
            }
            if long {
                tprintln!("total {}", "总计 {}", count);
                let user = current_user();
                for e in entries.into_iter().flatten() {
                    println!("  -rw-r--r-- 1 {} {} {:>8}  {}", user, user, e.size, e.name_str());
                }
            } else {
                vga::with_writer(|w| {
                    w.set_color(Color::LightCyan, Color::Black);
                    w.put_str(tr!("  NAME                              SIZE (bytes)\n",
                                  "  名称                              大小(字节)\n"));
                    w.set_color(Color::White, Color::Black);
                });
                for e in entries.into_iter().flatten() {
                    println!("  {:<32} {:>8}", e.name_str(), e.size);
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
    let mut numbered = false;
    let mut names: [&str; 8] = [""; 8];
    let mut n_names = 0usize;
    for p in rest.split_whitespace() {
        if p == "-n" {
            numbered = true;
        } else if n_names < 8 {
            names[n_names] = p;
            n_names += 1;
        }
    }
    if n_names == 0 {
        tprintln!("Usage: cat [-n] <file>...", "用法: cat [-n] <文件>...");
        return;
    }
    let mut buf = [0u8; crate::fs::MAX_FILE_SIZE];
    for (fi, name) in names[..n_names].iter().enumerate() {
        match crate::fs::read(name, &mut buf) {
            Some(n) => {
                if n_names > 1 {
                    println!("==> {} <==", name);
                }
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
        if fi + 1 < n_names { println!(""); }
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
    let mut force = false;
    for p in rest.split_whitespace() {
        if p == "-f" || p == "--force" || p == "-r" || p == "-rf" || p == "-fr" {
            force = true;
            continue;
        }
        match crate::fs::delete(p) {
            true => println!("removed '{}'", p),
            false => {
                if !force || crate::fs::find(p).is_some() {
                    tprintln!("rm: no such file: {}", "rm: 文件不存在: {}", p);
                }
            }
        }
    }
    if rest.trim().is_empty() {
        tprintln!("Usage: rm [-f] <file>...", "用法: rm [-f] <文件>...");
    }
}

// ===========================================================================
// 类 Linux 扩展命令
// ===========================================================================

fn cmd_echo(rest: &str) -> bool {
    let mut op: Option<(usize, usize, bool)> = None;
    if let Some(p) = rest.find(">>") {
        op = Some((p, 2, true));
    } else if let Some(p) = rest.find('>') {
        op = Some((p, 1, false));
    }
    let Some((pos, oplen, append)) = op else {
        println!("{}", rest);
        return false;
    };
    let text = rest[..pos].trim();
    let name = rest[pos + oplen..].trim();
    if name.is_empty() {
        tprintln!("echo: missing output file", "echo: 缺少输出文件");
        return false;
    }
    let text = text.trim_matches('"').trim_matches('\'');
    let mut out = [0u8; crate::fs::MAX_FILE_SIZE];
    let mut n = 0usize;
    if append {
        if let Some(old) = crate::fs::read(name, &mut out) {
            n = old;
            if n + text.len() + 1 > out.len() {
                tprintln!("echo: file would exceed 4 KiB", "echo: 文件将超过 4 KiB");
                return false;
            }
        }
    }
    let bytes = text.as_bytes();
    let take = bytes.len().min(out.len().saturating_sub(n + 1));
    out[n..n + take].copy_from_slice(&bytes[..take]);
    n += take;
    out[n] = b'\n';
    n += 1;
    if crate::fs::write(name, &out[..n]) {
        return false;
    }
    tprintln!("echo: write failed", "echo: 写入失败");
    false
}

fn cmd_date() {
    let t = rtc::now();
    if t.month == 0 || t.day == 0 || t.month > 12 {
        tprintln!("date: RTC unavailable", "date: 实时时钟不可用");
        return;
    }
    println!("{} {} {:02} {:02}:{:02}:{:02} {}",
        rtc::weekday_name(t.weekday), rtc::month_name(t.month), t.day,
        t.hour, t.minute, t.second, t.year);
}

fn two_args(rest: &str) -> (&str, &str) {
    let mut it = rest.splitn(2, char::is_whitespace);
    let a = it.next().unwrap_or("").trim();
    let b = it.next().unwrap_or("").trim();
    (a, b)
}

fn parse_n<'a>(rest: &'a str, def: usize) -> (usize, &'a str) {
    let t = rest.trim();
    if let Some(r) = t.strip_prefix("-n") {
        let mut it = r.split_whitespace();
        let n = it.next().and_then(|s| s.parse::<usize>().ok()).unwrap_or(def);
        (n, it.next().unwrap_or(""))
    } else {
        (def, t)
    }
}

fn cmd_head(rest: &str) {
    let (n, name) = parse_n(rest, 10);
    if name.is_empty() {
        tprintln!("Usage: head [-n N] <file>", "用法: head [-n N] <文件>");
        return;
    }
    let mut buf = [0u8; crate::fs::MAX_FILE_SIZE];
    match crate::fs::read(name, &mut buf) {
        Some(len) => {
            let text = core::str::from_utf8(&buf[..len]).unwrap_or("");
            for line in text.lines().take(n) {
                println!("{}", line);
            }
        }
        None => tprintln!("head: no such file: {}", "head: 文件不存在: {}", name),
    }
}

fn cmd_tail(rest: &str) {
    let (n, name) = parse_n(rest, 10);
    if name.is_empty() {
        tprintln!("Usage: tail [-n N] <file>", "用法: tail [-n N] <文件>");
        return;
    }
    let mut buf = [0u8; crate::fs::MAX_FILE_SIZE];
    match crate::fs::read(name, &mut buf) {
        Some(len) => {
            let text = core::str::from_utf8(&buf[..len]).unwrap_or("");
            let total = text.lines().count();
            let skip = total.saturating_sub(n);
            for line in text.lines().skip(skip) {
                println!("{}", line);
            }
        }
        None => tprintln!("tail: no such file: {}", "tail: 文件不存在: {}", name),
    }
}

fn cmd_wc(rest: &str) {
    let mut lines = false;
    let mut words = false;
    let mut bytes = false;
    let mut name = "";
    for p in rest.split_whitespace() {
        match p {
            "-l" => lines = true,
            "-w" => words = true,
            "-c" => bytes = true,
            _ => name = p,
        }
    }
    if name.is_empty() {
        tprintln!("Usage: wc [-l] [-w] [-c] <file>", "用法: wc [-l] [-w] [-c] <文件>");
        return;
    }
    let all = !lines && !words && !bytes;
    let mut buf = [0u8; crate::fs::MAX_FILE_SIZE];
    match crate::fs::read(name, &mut buf) {
        Some(n) => {
            let text = core::str::from_utf8(&buf[..n]).unwrap_or("");
            let lc = text.lines().count();
            let wc = text.split_whitespace().count();
            let mut out = alloc::string::String::new();
            if all || lines { let _ = write!(out, "{:>6} ", lc); }
            if all || words { let _ = write!(out, "{:>6} ", wc); }
            if all || bytes { let _ = write!(out, "{:>6} ", n); }
            let _ = write!(out, "{}", name);
            println!("{}", out);
        }
        None => tprintln!("wc: no such file: {}", "wc: 文件不存在: {}", name),
    }
}

fn cmd_cp(rest: &str) {
    let (src, dst) = two_args(rest);
    if src.is_empty() || dst.is_empty() {
        tprintln!("Usage: cp <source> <dest>", "用法: cp <源文件> <目标文件>");
        return;
    }
    let mut buf = [0u8; crate::fs::MAX_FILE_SIZE];
    match crate::fs::read(src, &mut buf) {
        Some(n) => {
            if crate::fs::write(dst, &buf[..n]) {
                tprintln!("copied {} -> {}", "已复制 {} -> {}", src, dst);
            } else {
                tprintln!("cp: write failed (disk full or name too long)", "cp: 写入失败（磁盘已满或文件名过长）");
            }
        }
        None => tprintln!("cp: no such file: {}", "cp: 文件不存在: {}", src),
    }
}

fn cmd_mv(rest: &str) {
    let (src, dst) = two_args(rest);
    if src.is_empty() || dst.is_empty() {
        tprintln!("Usage: mv <source> <dest>", "用法: mv <源文件> <目标文件>");
        return;
    }
    let mut buf = [0u8; crate::fs::MAX_FILE_SIZE];
    match crate::fs::read(src, &mut buf) {
        Some(n) => {
            if crate::fs::write(dst, &buf[..n]) && crate::fs::delete(src) {
                tprintln!("moved {} -> {}", "已移动 {} -> {}", src, dst);
            } else {
                tprintln!("mv: failed", "mv: 操作失败");
            }
        }
        None => tprintln!("mv: no such file: {}", "mv: 文件不存在: {}", src),
    }
}

fn cmd_find(rest: &str) {
    let pat = rest.trim();
    if pat.is_empty() {
        tprintln!("Usage: find <pattern>", "用法: find <模式>");
        return;
    }
    let mut found = 0usize;
    if let Some((entries, _)) = crate::fs::list() {
        for e in entries.iter().flatten() {
            if e.name_str().contains(pat) {
                println!("./{}", e.name_str());
                found += 1;
            }
        }
    }
    if found == 0 {
        tprintln!("find: no match for '{}'", "find: 未找到匹配 '{}'", pat);
    }
}

fn cmd_stat(rest: &str) {
    let name = rest.split_whitespace().next().unwrap_or("").trim();
    if name.is_empty() {
        tprintln!("Usage: stat <file>", "用法: stat <文件>");
        return;
    }
    match crate::fs::find(name) {
        Some(e) => {
            tprintln!("  File: {}", "  文件: {}", e.name_str());
            tprintln!("  Size: {} bytes", "  大小: {} 字节", e.size);
            tprintln!("  Slot: {}", "  槽位: {}", e.slot);
            tprintln!("  Type: regular file (MiniFS)", "  类型: 普通文件 (MiniFS)");
        }
        None => tprintln!("stat: no such file: {}", "stat: 文件不存在: {}", name),
    }
}

fn cmd_tree() {
    println!(".");
    if let Some((entries, count)) = crate::fs::list() {
        let mut i = 0usize;
        for e in entries.iter().flatten() {
            i += 1;
            let branch = if i == count { "`--" } else { "|--" };
            println!("{} {}", branch, e.name_str());
        }
    }
}

fn cmd_lspci() {
    println!("{}", tr!("Bus  Dev Func  Vendor Device Class", "总线 设备 功能  厂商   设备   类别"));
    let mut shown = 0usize;
    for bus in 0..8u8 {
        for dev in 0..32u8 {
            for func in 0..8u8 {
                let id = pci_read(bus, dev, func, 0x00);
                if id == 0xFFFF_FFFF { continue; }
                let class = pci_read(bus, dev, func, 0x08);
                println!("{:02X}   {:02X}  {:X}     {:04X}   {:04X}   {:02X}{:02X}",
                    bus, dev, func, id & 0xFFFF, id >> 16, (class >> 24) & 0xFF, (class >> 16) & 0xFF);
                shown += 1;
                if shown >= 24 { return; }
            }
        }
    }
}

fn pci_read(bus: u8, dev: u8, func: u8, reg: u8) -> u32 {
    let addr = 0x8000_0000u32 | ((bus as u32) << 16) | ((dev as u32) << 11)
        | ((func as u32) << 8) | ((reg & 0xFC) as u32);
    unsafe {
        crate::io::outl(0xCF8, addr);
        crate::io::inl(0xCFC)
    }
}

fn cmd_lscpu() {
    let info = crate::hw::check();
    let cpu = info.cpu;
    let mut b = alloc::string::String::new();
    let _ = write!(b, "{}", crate::hw::vendor_str(&cpu));
    println!("Architecture : x86_64");
    println!("Vendor       : {}", b);
    println!("Long mode    : {}", if cpu.long_mode { "yes" } else { "no" });
    println!("PAE          : {}", if cpu.pae { "yes" } else { "no" });
    println!("SSE2         : {}", if cpu.sse2 { "yes" } else { "no" });
    println!("APIC         : {}", if cpu.apic { "yes" } else { "no" });
}

fn cmd_dmesg() {
    let mem = crate::perf::mem_info();
    let disk = crate::perf::disk_info();
    let cpu = crate::hw::cpu();
    println!("[0.000000] MLOS 1.0.6 x86_64 booting");
    println!("[0.000000] CPU: {}", crate::hw::vendor_str(&cpu));
    println!("[0.000000] Memory: {} MiB usable", mem.total_kib / 1024);
    println!("[0.000000] Display: {}x{} VBE framebuffer", crate::fb::width(), crate::fb::height());
    if disk.present {
        println!("[0.000000] ata0: {} MiB ATA disk", disk.total_sectors / 2048);
    } else {
        println!("[0.000000] ata0: not detected");
    }
    println!("[0.000000] usb: {} controller(s)", crate::usb::count());
    println!("[0.000000] MiniFS: LBA 4 directory, 12 slots x 4 KiB");
}

fn cmd_env() {
    let user = current_user();
    println!("USER={}", user);
    println!("HOME=/home/{}", user);
    println!("PWD={}", cwd_str());
    println!("SHELL=/bin/mlsh");
    println!("TERM=linux-fb");
    println!("HOSTNAME=mlos");
    println!("LANG={}", if vga::lang_zh() { "zh_CN.UTF-8" } else { "en_US.UTF-8" });
}

fn cmd_hw() {
    let info = crate::hw::check();
    println!("{}", tr!("  === Hardware Requirements ===", "  === 硬件要求检测 ==="));
    println!("  CPU long mode : {}", if info.cpu.long_mode { "PASS" } else { "FAIL" });
    println!("  PAE           : {}", if info.cpu.pae { "PASS" } else { "FAIL" });
    println!("  SSE2          : {}", if info.cpu.sse2 { "PASS" } else { "FAIL" });
    println!("  Memory        : {} MiB (min 16 MiB)", info.mem_kib / 1024);
    println!("  Disk          : {} MiB (min 16 MiB)", info.disk_sectors as u64 / 2048);
    println!("  VBE framebuffer: {}x{}x{}", info.fb_w, info.fb_h, info.fb_bpp);
    println!("  GUI available : {}", if info.gui_ok { "yes" } else { "no" });
    println!("  Overall       : {}", if info.critical_ok { "PASS" } else { "FAIL" });
}

fn cmd_demo() {
    let text = "欢迎使用 MLOS！\n\n这是图形文件查看器中的中文点阵字体示例。\n布局方式：使用另一种中文渲染方式，直接向 VBE 帧缓冲绘制 16x16 点阵，\n不再改写 VGA 字模槽，所以不会出现字符码覆盖导致的乱码。\n\nUnicode UTF-8 / 中文标点：，。！？（）【】\nDeveloper: MEME-LMXDG\n";
    if crate::fs::write("welcome.txt", text.as_bytes()) {
        tprintln!("Created welcome.txt ({} bytes).", "已创建 welcome.txt（{} 字节）。", text.len());
        tprintln!("Run 'cat welcome.txt' or open Files in the desktop.", "可用 cat welcome.txt 查看，或在桌面打开文件管理器。");
    } else {
        tprintln!("demo: write failed", "demo: 写入失败");
    }
}

fn cmd_passwd() {
    let Some(mut cfg) = crate::storage::load() else {
        tprintln!("passwd: no configuration on disk", "passwd: 硬盘上没有配置");
        return;
    };
    let mut buf = [0u8; 64];
    tprint!("Current password: ", "当前密码: ");
    let n = read_line(&mut buf, Some(b'*'));
    let cur = core::str::from_utf8(&buf[..n]).unwrap_or("").trim();
    if crate::storage::fnv1a(cur.as_bytes()) != cfg.pass_hash {
        tprintln!("passwd: incorrect password", "passwd: 密码错误");
        return;
    }
    let mut nbuf = [0u8; 64];
    tprint!("New password: ", "新密码: ");
    let n1 = read_line(&mut nbuf, Some(b'*'));
    let p1 = core::str::from_utf8(&nbuf[..n1]).unwrap_or("").trim();
    if p1.is_empty() || p1.len() > 16 {
        tprintln!("passwd: password must be 1-16 characters", "passwd: 密码长度必须为 1-16 字符");
        return;
    }
    let mut cbuf = [0u8; 64];
    tprint!("Confirm new password: ", "确认新密码: ");
    let n2 = read_line(&mut cbuf, Some(b'*'));
    let p2 = core::str::from_utf8(&cbuf[..n2]).unwrap_or("").trim();
    if p1 != p2 {
        tprintln!("passwd: passwords do not match", "passwd: 两次密码不一致");
        return;
    }
    cfg.pass_hash = crate::storage::fnv1a(p1.as_bytes());
    if crate::storage::save(&cfg) {
        tprintln!("passwd: password updated", "passwd: 密码已更新");
    } else {
        tprintln!("passwd: failed to save", "passwd: 保存失败");
    }
}
