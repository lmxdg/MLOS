//! 实用小工具：计算器、进制转换、素数判断、哈希、随机数

use crate::input::read_line;
use crate::rng::Rng;
use crate::storage::fnv1a;
use crate::{print, println, tr, tprintln, tprint};
use crate::vga::{self, Color};
use core::fmt::Write;

/// 简单四则运算计算器（支持 + - * / 和括号，整数运算）
pub fn calculator(expr: &str) {
    let mut parser = ExprParser::new(expr.as_bytes());
    match parser.parse_expr() {
        Ok(v) => {
            vga::with_writer(|w| {
                w.set_color(Color::LightGreen, Color::Black);
                let _ = w.write_fmt(format_args!("  = {}\n", v));
                w.set_color(Color::White, Color::Black);
            });
        }
        Err(msg) => tprintln!("  Error: {}", "  错误: {}", msg),
    }
}

/// 交互式计算器
pub fn calculator_interactive() {
    println!("");
    vga::with_writer(|w| {
        w.set_color(Color::LightBlue, Color::Black);
        w.put_str(tr!("  === Calculator ===\n", "  === 计算器 ===\n"));
        w.set_color(Color::White, Color::Black);
    });
    tprintln!("  Supports + - * / and parentheses. Type 'q' to quit.\n",
              "  支持 + - * / 和括号，输入q退出。\n");
    let mut buf = [0u8; 128];
    loop {
        print!("  calc> ");
        let n = read_line(&mut buf, None);
        let expr = core::str::from_utf8(&buf[..n]).unwrap_or("").trim();
        if expr.eq_ignore_ascii_case("q") || expr.eq_ignore_ascii_case("quit") {
            return;
        }
        if expr.is_empty() {
            continue;
        }
        calculator(expr);
    }
}

/// 进制转换：输入十进制数，显示 hex/bin/oct
pub fn base_convert(arg: &str) {
    let arg = arg.trim();
    if arg.is_empty() {
        // 交互模式
        println!("");
        vga::with_writer(|w| {
            w.set_color(Color::LightBlue, Color::Black);
            w.put_str(tr!("  === Base Converter ===\n", "  === 进制转换 ===\n"));
            w.set_color(Color::White, Color::Black);
        });
        let mut buf = [0u8; 64];
        tprint!("  Enter a decimal number (or 'q'): ", "  输入十进制数(或q): ");
        let n = read_line(&mut buf, None);
        let s = core::str::from_utf8(&buf[..n]).unwrap_or("").trim();
        if s.eq_ignore_ascii_case("q") {
            return;
        }
        print_number(s);
    } else {
        print_number(arg);
    }
}

fn print_number(s: &str) {
    let val = match parse_u64(s) {
        Some(v) => v,
        None => {
            tprintln!("  Invalid number: {}", "  无效数字: {}", s);
            return;
        }
    };
    tprintln!("  DEC : {}", "  十进制 : {}", val);
    tprintln!("  HEX : 0x{}", "  十六进制 : 0x{}", to_radix(val, 16).as_str());
    tprintln!("  BIN : 0b{}", "  二进制 : 0b{}", to_radix(val, 2).as_str());
    tprintln!("  OCT : 0o{}", "  八进制 : 0o{}", to_radix(val, 8).as_str());
}

/// 素数判断
pub fn prime_check(arg: &str) {
    let mut buf = [0u8; 64];
    let s = if arg.trim().is_empty() {
        tprint!("  Enter a number: ", "  输入数字: ");
        let n = read_line(&mut buf, None);
        core::str::from_utf8(&buf[..n]).unwrap_or("").trim()
    } else {
        arg.trim()
    };
    match parse_u64(s) {
        Some(n) => {
            if is_prime(n) {
                vga::with_writer(|w| {
                    w.set_color(Color::LightGreen, Color::Black);
                });
                tprintln!("  {} is PRIME", "  {} 是素数", n);
                vga::with_writer(|w| { w.set_color(Color::White, Color::Black); });
            } else {
                tprintln!("  {} is not prime", "  {} 不是素数", n);
            }
        }
        None => tprintln!("  Invalid number: {}", "  无效数字: {}", s),
    }
}

/// 计算字符串的 FNV-1a 哈希
pub fn hash_text(arg: &str) {
    let mut buf = [0u8; 128];
    let s = if arg.trim().is_empty() {
        tprint!("  Enter text to hash: ", "  输入要哈希的文本: ");
        let n = read_line(&mut buf, None);
        core::str::from_utf8(&buf[..n]).unwrap_or("").trim()
    } else {
        arg.trim()
    };
    let h = fnv1a(s.as_bytes());
    println!("  FNV-1a(\"{}\") = 0x{:x}", s, h);
}

/// 生成随机数
pub fn random_number(arg: &str) {
    let max = parse_u64(arg.trim()).unwrap_or(100);
    let mut rng = Rng::new();
    let val = rng.range(1, max.max(2));
    vga::with_writer(|w| { w.set_color(Color::Yellow, Color::Black); });
    tprintln!("  Random [1, {}]: {}", "  随机数 [1, {}]: {}", max.max(2), val);
    vga::with_writer(|w| { w.set_color(Color::White, Color::Black); });
}

/// 显示 ASCII 可打印字符表
pub fn ascii_table() {
    println!("");
    vga::with_writer(|w| {
        w.set_color(Color::LightBlue, Color::Black);
        w.put_str(tr!("  === ASCII Table (32-126) ===\n", "  === ASCII 表 (32-126) ===\n"));
        w.set_color(Color::White, Color::Black);
    });
    // 每行显示 8 个字符：dec=char
    let mut c = 32u8;
    while c <= 126 {
        let mut line = StringBuf::new();
        for _ in 0..8 {
            if c <= 126 {
                let _ = line.write_fmt(format_args!("{:3}={:2}  ", c, c as char));
                c += 1;
            }
        }
        println!("  {}", line.as_str());
    }
}

/// 显示工具菜单
pub fn menu() {
    println!("");
    vga::with_writer(|w| {
        w.set_color(Color::LightBlue, Color::Black);
        w.put_str(tr!("  === Utility Tools ===\n", "  === 实用工具 ===\n"));
        w.set_color(Color::White, Color::Black);
    });
    tprintln!("  calc <expr>     Calculator (e.g. calc 2+3*4)", "  calc <表达式>   计算器(如 calc 2+3*4)");
    tprintln!("  base <number>   Convert decimal to hex/bin/oct", "  base <数字>    十进制转十六进制/二进制/八进制");
    tprintln!("  prime <number>  Check if a number is prime", "  prime <数字>   素数判断");
    tprintln!("  hash <text>     Compute FNV-1a hash", "  hash <文本>    FNV-1a哈希");
    tprintln!("  rand [max]      Generate a random number", "  rand [最大值]  生成随机数");
    tprintln!("  ascii           Show printable ASCII table", "  ascii          显示ASCII可打印表");
}

// ---------- 辅助函数 ----------

/// 简易表达式解析器：支持 + - * / ( ) 和正整数
struct ExprParser<'a> {
    input: &'a [u8],
    pos: usize,
}

impl<'a> ExprParser<'a> {
    fn new(input: &'a [u8]) -> Self {
        Self { input, pos: 0 }
    }

    fn parse_expr(&mut self) -> Result<i64, &'static str> {
        let mut val = self.parse_term()?;
        loop {
            self.skip_ws();
            if self.pos >= self.input.len() {
                break;
            }
            match self.input[self.pos] {
                b'+' => {
                    self.pos += 1;
                    let rhs = self.parse_term()?;
                    val = val.wrapping_add(rhs);
                }
                b'-' => {
                    self.pos += 1;
                    let rhs = self.parse_term()?;
                    val = val.wrapping_sub(rhs);
                }
                _ => break,
            }
        }
        Ok(val)
    }

    fn parse_term(&mut self) -> Result<i64, &'static str> {
        let mut val = self.parse_factor()?;
        loop {
            self.skip_ws();
            if self.pos >= self.input.len() {
                break;
            }
            match self.input[self.pos] {
                b'*' => {
                    self.pos += 1;
                    let rhs = self.parse_factor()?;
                    val = val.wrapping_mul(rhs);
                }
                b'/' => {
                    self.pos += 1;
                    let rhs = self.parse_factor()?;
                    if rhs == 0 {
                        return Err("division by zero");
                    }
                    val /= rhs;
                }
                _ => break,
            }
        }
        Ok(val)
    }

    fn parse_factor(&mut self) -> Result<i64, &'static str> {
        self.skip_ws();
        if self.pos >= self.input.len() {
            return Err("unexpected end of expression");
        }
        if self.input[self.pos] == b'(' {
            self.pos += 1;
            let val = self.parse_expr()?;
            self.skip_ws();
            if self.pos < self.input.len() && self.input[self.pos] == b')' {
                self.pos += 1;
                Ok(val)
            } else {
                Err("missing ')'")
            }
        } else {
            self.parse_number()
        }
    }

    fn parse_number(&mut self) -> Result<i64, &'static str> {
        self.skip_ws();
        let start = self.pos;
        while self.pos < self.input.len() && self.input[self.pos].is_ascii_digit() {
            self.pos += 1;
        }
        if start == self.pos {
            return Err("expected a number");
        }
        let mut val: i64 = 0;
        for &b in &self.input[start..self.pos] {
            val = val
                .checked_mul(10)
                .and_then(|v| v.checked_add((b - b'0') as i64))
                .ok_or("number too large")?;
        }
        Ok(val)
    }

    fn skip_ws(&mut self) {
        while self.pos < self.input.len() && self.input[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
    }
}

/// 栈上小型字符串缓冲，实现 fmt::Write（避免依赖 alloc）
struct StringBuf {
    data: [u8; 128],
    len: usize,
}

impl StringBuf {
    fn new() -> Self {
        Self { data: [0; 128], len: 0 }
    }
    fn as_str(&self) -> &str {
        core::str::from_utf8(&self.data[..self.len]).unwrap_or("")
    }
}

impl core::fmt::Write for StringBuf {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        for b in s.bytes() {
            if self.len < self.data.len() {
                self.data[self.len] = b;
                self.len += 1;
            }
        }
        Ok(())
    }
}

/// 解析 u64（小工具用）
fn parse_u64(s: &str) -> Option<u64> {
    if s.is_empty() {
        return None;
    }
    let mut val: u64 = 0;
    for b in s.bytes() {
        if !b.is_ascii_digit() {
            return None;
        }
        val = val.checked_mul(10)?.checked_add((b - b'0') as u64)?;
    }
    Some(val)
}

/// 数字转任意进制（2~36）字符串
fn to_radix(mut n: u64, radix: u64) -> StringBuf {
    let mut out = StringBuf::new();
    if n == 0 {
        let _ = out.write_str("0");
        return out;
    }
    let digits = b"0123456789abcdef";
    let mut tmp = [0u8; 64];
    let mut len = 0;
    while n > 0 {
        tmp[len] = digits[(n % radix) as usize];
        len += 1;
        n /= radix;
    }
    // 反转
    for i in (0..len).rev() {
        let _ = out.write_str(core::str::from_utf8(&tmp[i..i + 1]).unwrap_or(""));
    }
    out
}

/// 素数判断
fn is_prime(n: u64) -> bool {
    if n < 2 {
        return false;
    }
    if n == 2 {
        return true;
    }
    if n % 2 == 0 {
        return false;
    }
    let mut d = 3u64;
    while d * d <= n {
        if n % d == 0 {
            return false;
        }
        d += 2;
    }
    true
}
