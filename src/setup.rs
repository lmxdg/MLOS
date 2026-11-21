//! 安装向导与用户认证（v1.0.6 全新流程）
//!
//! 首次启动分 6 步完成：选择语言 → 硬件检测 → 硬盘检测 → 创建账户 → 安装系统 → 完成。
//! 安装阶段用旋转动画反馈进度；配置写入 ATA 硬盘持久化。
//! 后续启动时需要登录认证。

use crate::input::{read_line, wait_key};
use crate::storage::{self, Config};
use crate::{println, tr, tprintln, tprint};
use crate::vga::{self, Color};

const STEPS: usize = 6;

/// 判断是否已安装（硬盘上有有效配置）
pub fn is_installed() -> bool {
    match storage::load() {
        Some(cfg) => cfg.installed,
        None => false,
    }
}

fn step_header(n: usize, en: &str, zh: &str) {
    vga::with_writer(|w| {
        w.set_color(Color::LightCyan, Color::Black);
        let _ = w.write_fmt(format_args!("\n[Step {}/{}] ", n, STEPS));
        w.put_str(tr!(en, zh));
        w.put_str("\n");
        w.set_color(Color::White, Color::Black);
    });
}

fn rule() {
    vga::with_writer(|w| {
        w.set_color(Color::DarkGray, Color::Black);
        w.put_str("  --------------------------------------------------------\n");
        w.set_color(Color::White, Color::Black);
    });
}

fn hw_line(label: &str, ok: bool, detail: &str) {
    vga::with_writer(|w| {
        if ok {
            w.set_color(Color::LightGreen, Color::Black);
            w.put_str("  [PASS] ");
        } else {
            w.set_color(Color::LightRed, Color::Black);
            w.put_str("  [FAIL] ");
        }
        w.set_color(Color::LightGray, Color::Black);
        w.put_str(label);
        w.set_color(Color::DarkGray, Color::Black);
        w.put_str("  ");
        w.put_str(detail);
        w.put_str("\n");
        w.set_color(Color::White, Color::Black);
    });
}

/// 运行安装向导，返回配置结果
pub fn run_wizard() -> Option<Config> {
    vga::with_writer(|w| w.clear());
    vga::with_writer(|w| {
        w.set_color(Color::LightCyan, Color::Black);
        w.put_str("  +--------------------------------------------------+\n");
        w.put_str(tr!("  |        MLOS Setup Wizard  v1.0.6                |\n",
                      "  |        MLOS 安装向导  v1.0.6                    |\n"));
        w.put_str("  +--------------------------------------------------+\n");
        w.set_color(Color::White, Color::Black);
    });

    // ---- Step 1: 语言选择 ----
    step_header(1,
        "Choose your language",
        "选择语言");
    tprintln!("  1) English", "  1) 英文");
    tprintln!("  2) Chinese (Preview)", "  2) 中文（预览版）");
    tprint!("  Select [1]: ", "  请选择 [1]: ");
    let mut lbuf = [0u8; 8];
    let ln = read_line(&mut lbuf, None);
    let choice = core::str::from_utf8(&lbuf[..ln]).unwrap_or("").trim();
    let zh = choice == "2";
    vga::set_lang(zh);
    // 切换语言会清屏，重新绘制向导标题
    vga::with_writer(|w| {
        w.set_color(Color::LightCyan, Color::Black);
        w.put_str("  +--------------------------------------------------+\n");
        w.put_str(tr!("  |        MLOS Setup Wizard  v1.0.6                |\n",
                      "  |        MLOS 安装向导  v1.0.6                    |\n"));
        w.put_str("  +--------------------------------------------------+\n");
        w.set_color(Color::White, Color::Black);
    });
    step_header(1,
        "Choose your language",
        "选择语言");
    tprintln!("  -> {}", "  -> {}", if zh { "中文（预览版）" } else { "English" });
    rule();

    // ---- Step 2: 硬件要求检测 ----
    step_header(2,
        "Checking hardware requirements",
        "检查硬件要求");
    let hw = crate::hw::check();
    let cpu_ok = hw.cpu.long_mode && hw.cpu.pae && hw.cpu.sse2;
    hw_line(tr!("CPU long mode / PAE / SSE2", "CPU 长模式 / PAE / SSE2"), cpu_ok,
        if cpu_ok { tr!("supported", "支持") } else { tr!("missing required CPU feature", "缺少必需的 CPU 特性") });
    let mem_txt = if hw.mem_kib > 0 {
        if hw.mem_ok { tr!("enough", "容量充足") } else { tr!("too small (need >= 16 MiB)", "容量不足（最低 16 MiB）") }
    } else { tr!("unknown (no memory map)", "未知（无内存映射）") };
    hw_line(tr!("Usable memory", "可用内存"), hw.mem_ok, mem_txt);
    hw_line(tr!("PS/2 keyboard controller", "PS/2 键盘控制器"), hw.ps2_ok,
        if hw.ps2_ok { tr!("ready", "就绪") } else { tr!("not detected", "未检测到") });
    if hw.fb_present {
        let mut buf = alloc::string::String::new();
        if vga::lang_zh() {
            let _ = core::fmt::Write::write_fmt(&mut buf, format_args!(
                "{}x{}x{} VBE 帧缓冲", hw.fb_w, hw.fb_h, hw.fb_bpp));
        } else {
            let _ = core::fmt::Write::write_fmt(&mut buf, format_args!(
                "{}x{}x{} VBE framebuffer", hw.fb_w, hw.fb_h, hw.fb_bpp));
        }
        hw_line(tr!("Graphical display (GUI)", "图形显示 (GUI)"), hw.fb_ok, &buf);
    } else {
        hw_line(tr!("Graphical display (GUI)", "图形显示 (GUI)"), false,
            tr!("VGA text mode only", "仅 VGA 文本模式"));
    }
    if hw.disk_present {
        let mut buf = alloc::string::String::new();
        let _ = core::fmt::Write::write_fmt(&mut buf, format_args!(
            "{} MiB ({} 16 MiB)", hw.disk_sectors as u64 / 2048,
            tr!("need >=", "最低")));
        hw_line(tr!("ATA hard disk", "ATA 硬盘"), hw.disk_ok, &buf);
    } else {
        hw_line(tr!("ATA hard disk", "ATA 硬盘"), false,
            tr!("not detected", "未检测到"));
    }
    if !hw.critical_ok {
        println!("");
        vga::with_writer(|w| w.set_color(Color::LightRed, Color::Black));
        tprintln!("  ERROR: this machine does not meet the minimum requirements.",
                  "  错误: 本机不满足最低硬件要求。");
        vga::with_writer(|w| w.set_color(Color::White, Color::Black));
        tprintln!("  CPU: x86_64 with PAE and SSE2, 16 MiB RAM, 16 MiB ATA disk.",
                  "  CPU: 支持 PAE/SSE2 的 x86_64，16 MiB 内存，16 MiB ATA 硬盘。");
        tprintln!("  Press any key to halt...", "  按任意键停机...");
        wait_key();
        return None;
    }
    if !hw.gui_ok {
        vga::with_writer(|w| w.set_color(Color::Yellow, Color::Black));
        tprintln!("  WARNING: no VBE framebuffer; desktop GUI will be unavailable.",
                  "  警告: 未检测到 VBE 帧缓冲，图形桌面不可用。");
        vga::with_writer(|w| w.set_color(Color::White, Color::Black));
    }
    tprintln!("  Hardware check: PASS", "  硬件检测: 通过");
    rule();

    // ---- Step 3: 硬盘检测 ----
    step_header(3,
        "Detecting hard disk",
        "检测硬盘");
    if !crate::ata::detect() {
        vga::with_writer(|w| {
            w.set_color(Color::LightRed, Color::Black);
            w.put_str(tr!("  ERROR: No hard disk detected!\n", "  错误: 未检测到硬盘！\n"));
            w.set_color(Color::White, Color::Black);
        });
        tprintln!("  MLOS needs a hard disk to store user data.", "  MLOS 需要硬盘来保存用户数据。");
        tprintln!("  Attach an IDE disk, then reboot.", "  请挂载 IDE 硬盘后重启。");
        tprintln!("  Press any key to halt...", "  按任意键停机...");
        wait_key();
        return None;
    }
    tprintln!("  Hard disk detected: OK", "  硬盘检测: 正常");
    rule();

    // ---- Step 4: 创建账户 ----
    step_header(4,
        "Create your account",
        "创建账户");
    tprintln!("  Your settings will be saved to the hard disk.\n", "  您的设置将保存到硬盘。\n");
    let mut buf = [0u8; 64];

    let username = loop {
        tprint!("  Enter username (1-16 chars): ", "  输入用户名 (1-16 字符): ");
        let n = read_line(&mut buf, None);
        let name = core::str::from_utf8(&buf[..n]).unwrap_or("").trim();
        if name.is_empty() {
            tprintln!("  Username cannot be empty.", "  用户名不能为空。");
            continue;
        }
        if name.len() > 16 {
            tprintln!("  Username too long (max 16 chars).", "  用户名太长（最多 16 字符）。");
            continue;
        }
        if !name.bytes().all(|b| (0x20..=0x7E).contains(&b)) {
            tprintln!("  Username contains invalid characters.", "  用户名包含非法字符。");
            continue;
        }
        let mut arr = [0u8; 32];
        arr[..name.len()].copy_from_slice(name.as_bytes());
        break arr;
    };

    let pass_hash = loop {
        let mut p1buf = [0u8; 64];
        tprint!("  Enter password (1-16 chars): ", "  输入密码 (1-16 字符): ");
        let n1 = read_line(&mut p1buf, Some(b'*'));
        let p1 = core::str::from_utf8(&p1buf[..n1]).unwrap_or("").trim();
        if p1.is_empty() {
            tprintln!("  Password cannot be empty.", "  密码不能为空。");
            continue;
        }
        if p1.len() > 16 {
            tprintln!("  Password too long (max 16 chars).", "  密码太长（最多 16 字符）。");
            continue;
        }
        let mut p2buf = [0u8; 64];
        tprint!("  Confirm password: ", "  确认密码: ");
        let n2 = read_line(&mut p2buf, Some(b'*'));
        let p2 = core::str::from_utf8(&p2buf[..n2]).unwrap_or("").trim();
        if p1 != p2 {
            tprintln!("  Passwords do not match. Try again.", "  两次密码不一致，请重试。");
            continue;
        }
        break storage::fnv1a(p1.as_bytes());
    };
    rule();

    // ---- Step 5: 安装系统 ----
    step_header(5,
        "Installing MLOS to disk",
        "正在把 MLOS 安装到硬盘");
    tprintln!("  Saving account settings...", "  正在保存账户设置...");
    let cfg = Config { username, pass_hash, installed: true, lang_zh: zh };
    if !storage::save(&cfg) {
        vga::with_writer(|w| {
            w.set_color(Color::LightRed, Color::Black);
            w.put_str(tr!("  FAILED: cannot write configuration.\n", "  失败: 无法写入配置。\n"));
            w.set_color(Color::White, Color::Black);
        });
        tprintln!("  Setup cannot continue.", "  安装无法继续。");
        tprintln!("  Press any key to halt...", "  按任意键停机...");
        wait_key();
        return None;
    }

    tprintln!("  Writing bootloader + kernel (spinning)...", "  正在写入引导程序与内核（转圈中）...");
    // 安装进程会在固定位置旋转，真实反映写入进度
    vga::with_writer(|w| {
        w.set_pos(21, 2);
        w.set_color(Color::DarkGray, Color::Black);
        w.put_str("  status: ");
        w.set_color(Color::Yellow, Color::Black);
    });
    match crate::installer::install() {
        Ok(n) => {
            vga::with_writer(|w| {
                w.set_pos(21, 2);
                w.set_color(Color::LightGreen, Color::Black);
                let _ = w.write_fmt(format_args!("  status: OK ({} sectors)\n", n));
                w.set_color(Color::White, Color::Black);
            });
        }
        Err(e) => {
            vga::with_writer(|w| {
                w.set_pos(21, 2);
                w.set_color(Color::LightRed, Color::Black);
                let _ = w.write_fmt(format_args!("  status: FAILED ({})\n", e));
                w.set_color(Color::White, Color::Black);
            });
        }
    }
    let verified = storage::load().map(|c| c.installed).unwrap_or(false);
    if !verified {
        tprintln!("  WARNING: could not verify saved data.", "  警告: 无法验证已保存的数据。");
    }
    rule();

    // ---- Step 6: 完成 ----
    step_header(6,
        "Setup complete",
        "安装完成");
    vga::with_writer(|w| {
        w.set_color(Color::LightGreen, Color::Black);
        w.put_str(tr!("  MLOS is ready. Enjoy!\n", "  MLOS 已就绪，祝您使用愉快！\n"));
        w.set_color(Color::White, Color::Black);
    });
    tprintln!("  The disk can now boot MLOS without the ISO.", "  现在无需 ISO 即可从硬盘启动 MLOS。");
    tprintln!("  Press any key to continue to login...", "  按任意键继续登录...");
    wait_key();

    Some(cfg)
}

/// 登录认证，成功返回 true
pub fn login(cfg: &Config) -> bool {
    vga::with_writer(|w| w.clear());
    println!("");
    vga::with_writer(|w| {
        w.set_color(Color::LightCyan, Color::Black);
        w.put_str("  +--------------------------------------------------+\n");
        w.put_str(tr!("  |                  MLOS Login                     |\n", "  |                  MLOS 登录                      |\n"));
        w.put_str("  +--------------------------------------------------+\n");
        w.set_color(Color::White, Color::Black);
    });
    println!("");

    let mut attempts = 0u32;

    loop {
        let mut ubuf = [0u8; 64];
        tprint!("  Username: ", "  用户名: ");
        let n = read_line(&mut ubuf, None);
        let user = core::str::from_utf8(&ubuf[..n]).unwrap_or("").trim();

        let mut pbuf = [0u8; 64];
        tprint!("  Password: ", "  密码: ");
        let n = read_line(&mut pbuf, Some(b'*'));
        let pass = core::str::from_utf8(&pbuf[..n]).unwrap_or("").trim();

        let user_ok = user == cfg.user_str();
        let pass_ok = storage::fnv1a(pass.as_bytes()) == cfg.pass_hash;

        if user_ok && pass_ok {
            println!("");
            vga::with_writer(|w| {
                w.set_color(Color::LightGreen, Color::Black);
                let _ = w.write_fmt(format_args!("{}", tr!("  Welcome back, ", "  欢迎回来， ")));
                let _ = w.write_fmt(format_args!("{}!\n", user));
                w.set_color(Color::White, Color::Black);
            });
            return true;
        }

        attempts += 1;
        println!("");
        vga::with_writer(|w| {
            w.set_color(Color::LightRed, Color::Black);
            w.put_str(tr!("  Invalid username or password.\n", "  用户名或密码错误。\n"));
            w.set_color(Color::White, Color::Black);
        });
        if attempts >= 5 {
            tprintln!("  Too many failed attempts. Halting.", "  失败次数过多，系统停机。");
            return false;
        }
        tprintln!("  Attempts remaining: {}\n", "  剩余尝试次数: {}\n", 5 - attempts);
    }
}
