//! 安装向导与用户认证
//!
//! 首次启动时引导用户设置用户名和密码，配置写入硬盘持久化。
//! 后续启动时需要登录认证。

use crate::input::{read_line, wait_key};
use crate::storage::{self, Config};
use core::fmt::Write;
use crate::{print, println};
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
        w.put_str("  |        MLOS First-Time Setup Wizard             |\n");
        w.put_str("  +--------------------------------------------------+\n");
        w.set_color(Color::White, Color::Black);
    });
    println!("  Welcome to MLOS! Let's set up your account.");
    println!("  Your settings will be saved to the hard disk.\n");
    // 检测硬盘
    if !crate::ata::detect() {
        vga::with_writer(|w| {
            w.set_color(Color::LightRed, Color::Black);
            w.put_str("  ERROR: No hard disk detected!\n");
            w.set_color(Color::White, Color::Black);
        });
        println!("  MLOS needs a hard disk to store user data.");
        println!("  Make sure a virtual/real IDE disk is attached.");
        println!("  Press any key to halt...");
        wait_key();
        return None;
    }
    println!("  Hard disk detected: OK\n");

    let mut buf = [0u8; 64];

    // 输入用户名
    let username = loop {
        print!("  Enter username (1-16 chars): ");
        let n = read_line(&mut buf, None);
        let name = core::str::from_utf8(&buf[..n]).unwrap_or("").trim();
        if name.is_empty() {
            println!("  Username cannot be empty.");
            continue;
        }
        if name.len() > 16 {
            println!("  Username too long (max 16 chars).");
            continue;
        }
        // 检查是否只包含可打印 ASCII
        if !name.bytes().all(|b| (0x20..=0x7E).contains(&b)) {
            println!("  Username contains invalid characters.");
            continue;
        }
        let mut arr = [0u8; 32];
        arr[..name.len()].copy_from_slice(name.as_bytes());
        break arr;
    };

    // 输入密码 + 确认
    let pass_hash = loop {
        let mut p1buf = [0u8; 64];
        print!("  Enter password (1-16 chars): ");
        let n1 = read_line(&mut p1buf, Some(b'*'));
        let p1 = core::str::from_utf8(&p1buf[..n1]).unwrap_or("").trim();
        if p1.is_empty() {
            println!("  Password cannot be empty.");
            continue;
        }
        if p1.len() > 16 {
            println!("  Password too long (max 16 chars).");
            continue;
        }
        let mut p2buf = [0u8; 64];
        print!("  Confirm password: ");
        let n2 = read_line(&mut p2buf, Some(b'*'));
        let p2 = core::str::from_utf8(&p2buf[..n2]).unwrap_or("").trim();
        if p1 != p2 {
            println!("  Passwords do not match. Try again.");
            continue;
        }
        break storage::fnv1a(p1.as_bytes());
    };

    // 保存到硬盘
    print!("  Saving configuration to disk... ");
    let cfg = Config {
        username,
        pass_hash,
        installed: true,
    };
    if storage::save(&cfg) {
        println!("OK");
    } else {
        vga::with_writer(|w| {
            w.set_color(Color::LightRed, Color::Black);
            w.put_str("FAILED\n");
            w.set_color(Color::White, Color::Black);
        });
        println!("  Could not write to disk. Setup cannot continue.");
        println!("  Press any key to halt...");
        wait_key();
        return None;
    }

    // 验证读回
    let verified = storage::load().map(|c| c.installed).unwrap_or(false);
    if !verified {
        println!("  WARNING: Could not verify saved data.");
    }

    println!("");
    vga::with_writer(|w| {
        w.set_color(Color::LightGreen, Color::Black);
        w.put_str("  Setup complete! Your account is ready.\n");
        w.set_color(Color::White, Color::Black);
    });
    println!("  Press any key to continue to login...");
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
        w.put_str("  |                  MLOS Login                     |\n");
        w.put_str("  +--------------------------------------------------+\n");
        w.set_color(Color::White, Color::Black);
    });
    println!("");

    let mut attempts = 0u32;

    loop {
        let mut ubuf = [0u8; 64];
        print!("  Username: ");
        let n = read_line(&mut ubuf, None);
        let user = core::str::from_utf8(&ubuf[..n]).unwrap_or("").trim();

        let mut pbuf = [0u8; 64];
        print!("  Password: ");
        let n = read_line(&mut pbuf, Some(b'*'));
        let pass = core::str::from_utf8(&pbuf[..n]).unwrap_or("").trim();

        let user_ok = user == cfg.user_str();
        let pass_ok = storage::fnv1a(pass.as_bytes()) == cfg.pass_hash;

        if user_ok && pass_ok {
            println!("");
            vga::with_writer(|w| {
                w.set_color(Color::LightGreen, Color::Black);
                let _ = w.write_fmt(format_args!("  Welcome back, {}!\n", user));
                w.set_color(Color::White, Color::Black);
            });
            return true;
        }

        attempts += 1;
        println!("");
        vga::with_writer(|w| {
            w.set_color(Color::LightRed, Color::Black);
            w.put_str("  Invalid username or password.\n");
            w.set_color(Color::White, Color::Black);
        });
        if attempts >= 5 {
            println!("  Too many failed attempts. Halting.");
            return false;
        }
        println!("  Attempts remaining: {}\n", 5 - attempts);
    }
}
