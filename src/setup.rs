//! 安装向导与用户认证
//!
//! 首次启动时引导用户设置用户名和密码，配置写入硬盘持久化。
//! 后续启动时需要登录认证。

use crate::input::{read_line, wait_key};
use crate::storage::{self, Config};
use core::fmt::Write;
use crate::{print, println, tr, tprintln, tprint};
use crate::vga::{self, Color};

/// 判断是否已安装（硬盘上有有效配置）
pub fn is_installed() -> bool {
    match storage::load() {
        Some(cfg) => cfg.installed,
        None => false,
    }
}

/// 运行安装向导，返回配置结果
pub fn run_wizard() -> Option<Config> {
    vga::with_writer(|w| w.clear());
    println!("");
    vga::with_writer(|w| {
        w.set_color(Color::LightCyan, Color::Black);
        w.put_str("  +--------------------------------------------------+\n");
        w.put_str(tr!("  |        MLOS First-Time Setup Wizard             |\n", "  |        MLOS 首次安装向导                       |\n"));
        w.put_str("  +--------------------------------------------------+\n");
        w.set_color(Color::White, Color::Black);
    });
    tprintln!("  Welcome to MLOS! Let's set up your account.", "  欢迎使用 MLOS！请设置您的账户。");
    tprintln!("  Your settings will be saved to the hard disk.\n", "  您的设置将保存到硬盘。\n");
    // 检测硬盘
    if !crate::ata::detect() {
        vga::with_writer(|w| {
            w.set_color(Color::LightRed, Color::Black);
            w.put_str("  ERROR: No hard disk detected!\n");
            w.set_color(Color::White, Color::Black);
        });
        tprintln!("  MLOS needs a hard disk to store user data.", "  MLOS 需要硬盘来保存用户数据。");
        tprintln!("  Make sure a virtual/real IDE disk is attached.", "  请确保已挂载虚拟或真实 IDE 硬盘。");
        tprintln!("  Press any key to halt...", "  按任意键停机...");
        wait_key();
        return None;
    }
    tprintln!("  Hard disk detected: OK\n", "  硬盘检测: 正常\n");

    let mut buf = [0u8; 64];

    // 输入用户名
    let username = loop {
        tprint!("  Enter username (1-16 chars): ", "  输入用户名(1-16字符): ");
        let n = read_line(&mut buf, None);
        let name = core::str::from_utf8(&buf[..n]).unwrap_or("").trim();
        if name.is_empty() {
            tprintln!("  Username cannot be empty.", "  用户名不能为空。");
            continue;
        }
        if name.len() > 16 {
            tprintln!("  Username too long (max 16 chars).", "  用户名太长(最多16字符)。");
            continue;
        }
        // 检查是否只包含可打印 ASCII
        if !name.bytes().all(|b| (0x20..=0x7E).contains(&b)) {
            tprintln!("  Username contains invalid characters.", "  用户名包含非法字符。");
            continue;
        }
        let mut arr = [0u8; 32];
        arr[..name.len()].copy_from_slice(name.as_bytes());
        break arr;
    };

    // 输入密码 + 确认
    let pass_hash = loop {
        let mut p1buf = [0u8; 64];
        tprint!("  Enter password (1-16 chars): ", "  输入密码(1-16字符): ");
        let n1 = read_line(&mut p1buf, Some(b'*'));
        let p1 = core::str::from_utf8(&p1buf[..n1]).unwrap_or("").trim();
        if p1.is_empty() {
            tprintln!("  Password cannot be empty.", "  密码不能为空。");
            continue;
        }
        if p1.len() > 16 {
            tprintln!("  Password too long (max 16 chars).", "  密码太长(最多16字符)。");
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

    // 保存到硬盘
    tprint!("  Saving configuration to disk... ", "  正在保存配置到硬盘... ");
    let cfg = Config {
        username,
        pass_hash,
        installed: true,
    };
    if storage::save(&cfg) {
        tprintln!("OK", "完成");
    } else {
        vga::with_writer(|w| {
            w.set_color(Color::LightRed, Color::Black);
            w.put_str(tr!("FAILED\n", "失败\n"));
            w.set_color(Color::White, Color::Black);
        });
        tprintln!("  Could not write to disk. Setup cannot continue.", "  无法写入硬盘，安装无法继续。");
        tprintln!("  Press any key to halt...", "  按任意键停机...");
        wait_key();
        return None;
    }

    // 验证读回
    let verified = storage::load().map(|c| c.installed).unwrap_or(false);
    if !verified {
        tprintln!("  WARNING: Could not verify saved data.", "  警告: 无法验证已保存的数据。");
    }

    println!("");
    vga::with_writer(|w| {
        w.set_color(Color::LightGreen, Color::Black);
        w.put_str(tr!("  Setup complete! Your account is ready.\n", "  安装完成！账户已就绪。\n"));
        w.set_color(Color::White, Color::Black);
    });
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
