//! 终端小游戏集合：数字炸弹、猜数字、石头剪刀布

use crate::input::{read_line, wait_key};
use crate::rng::Rng;
use core::fmt::Write;
use crate::{println, print};
use crate::vga::{self, Color};

/// 数字炸弹：1~100 中有一个"炸弹"数字，玩家输入逐步逼近，
/// 系统提示"更安全/更危险"，踩中炸弹即失败。
pub fn number_bomb() {
    println!("");
    vga::with_writer(|w| {
        w.set_color(Color::LightRed, Color::Black);
        w.put_str("  === Number Bomb ===\n");
        w.set_color(Color::White, Color::Black);
    });
    println!("  A bomb is hidden in [1, 100]. Enter numbers to narrow it down.");
    println!("  The closer you get to the bomb, the more dangerous it becomes.");
    println!("  Type 'q' to quit. Good luck!\n");

    let mut rng = Rng::new();
    let bomb = rng.range(1, 100) as u32;
    let mut buf = [0u8; 32];
    let mut tries = 0u32;

    loop {
        print!("  Try #{} | Guess (1-100): ", tries + 1);
        let n = read_line(&mut buf, None);
        let input = core::str::from_utf8(&buf[..n]).unwrap_or("").trim();

        if input.eq_ignore_ascii_case("q") || input.eq_ignore_ascii_case("quit") {
            println!("  You chicken out. The bomb was {}.", bomb);
            return;
        }

        let guess = match parse_u32(input) {
            Some(v) if v >= 1 && v <= 100 => v,
            _ => {
                println!("  Please enter a number between 1 and 100.");
                continue;
            }
        };
        tries += 1;

        let diff = if guess > bomb { guess - bomb } else { bomb - guess };

        if diff == 0 {
            vga::with_writer(|w| {
                w.set_color(Color::LightRed, Color::Black);
                w.put_str("  *** BOOM! You hit the bomb! Game over. ***\n");
                w.set_color(Color::White, Color::Black);
            });
            println!("  The bomb was {} and it took you {} tries.", bomb, tries);
            println!("  Press any key to continue...");
            wait_key();
            return;
        }

        let danger = if diff >= 40 {
            0
        } else if diff >= 25 {
            1
        } else if diff >= 12 {
            2
        } else if diff >= 5 {
            3
        } else {
            4
        };

        let (label, color) = match danger {
            0 => ("SAFE ...  (far away)", Color::LightGreen),
            1 => ("Warm ...", Color::Green),
            2 => ("Getting warm ...", Color::Yellow),
            3 => ("Hot! ...", Color::LightRed),
            _ => ("VERY HOT! ...", Color::Red),
        };

        vga::with_writer(|w| {
            w.set_color(color, Color::Black);
            let _ = w.write_fmt(format_args!("  {} [distance {}]\n", label, diff));
            w.set_color(Color::White, Color::Black);
        });
    }
}

/// 猜数字：经典 1~100 猜数字，提示大/小
pub fn guess_number() {
    println!("");
    vga::with_writer(|w| {
        w.set_color(Color::LightCyan, Color::Black);
        w.put_str("  === Guess the Number ===\n");
        w.set_color(Color::White, Color::Black);
    });
    println!("  I'm thinking of a number between 1 and 100.");
    println!("  Type 'q' to quit.\n");

    let mut rng = Rng::new();
    let secret = rng.range(1, 100) as u32;
    let mut buf = [0u8; 32];
    let mut tries = 0u32;

    loop {
        print!("  Your guess: ");
        let n = read_line(&mut buf, None);
        let input = core::str::from_utf8(&buf[..n]).unwrap_or("").trim();

        if input.eq_ignore_ascii_case("q") || input.eq_ignore_ascii_case("quit") {
            println!("  It was {}. Maybe next time!", secret);
            return;
        }

        let guess = match parse_u32(input) {
            Some(v) if v >= 1 && v <= 100 => v,
            _ => {
                println!("  Enter a number between 1 and 100.");
                continue;
            }
        };
        tries += 1;

        if guess < secret {
            println!("  Too small! Try higher.");
        } else if guess > secret {
            println!("  Too big! Try lower.");
        } else {
            vga::with_writer(|w| {
                w.set_color(Color::LightGreen, Color::Black);
                w.put_str("  Correct! You got it!\n");
                w.set_color(Color::White, Color::Black);
            });
            println!("  The number was {} and you found it in {} tries.", secret, tries);
            println!("  Press any key to continue...");
            wait_key();
            return;
        }
    }
}

/// 石头剪刀布：与电脑对战，五局三胜
pub fn rock_paper_scissors() {
    println!("");
    vga::with_writer(|w| {
        w.set_color(Color::Pink, Color::Black);
        w.put_str("  === Rock Paper Scissors ===\n");
        w.set_color(Color::White, Color::Black);
    });
    println!("  Type r (rock), p (paper), s (scissors), or q to quit.");
    println!("  First to 3 wins!\n");

    let mut rng = Rng::new();
    let mut buf = [0u8; 32];
    let mut player_wins = 0u32;
    let mut cpu_wins = 0u32;

    while player_wins < 3 && cpu_wins < 3 {
        print!("  Your move [r/p/s/q]: ");
        let n = read_line(&mut buf, None);
        let input = core::str::from_utf8(&buf[..n]).unwrap_or("").trim();
        let first = input.as_bytes().first().copied().unwrap_or(0) as char;

        let player = match first.to_ascii_lowercase() {
            'r' => 0,
            'p' => 1,
            's' => 2,
            'q' => {
                println!("  Quit. Score: You {} - {} CPU", player_wins, cpu_wins);
                return;
            }
            _ => {
                println!("  Invalid move. Use r, p, s, or q.");
                continue;
            }
        };

        let cpu = rng.range(0, 3) as usize;
        let names = ["rock", "paper", "scissors"];
        println!("  You: {}   CPU: {}", names[player], names[cpu]);

        // (player + 1) % 3 beats player; 若 cpu == (player+1)%3 则 CPU 胜
        if player == cpu {
            println!("  -> Draw!");
        } else if cpu == (player + 1) % 3 {
            println!("  -> CPU wins this round!");
            cpu_wins += 1;
        } else {
            println!("  -> You win this round!");
            player_wins += 1;
        }
        println!("  Score: You {} - {} CPU\n", player_wins, cpu_wins);
    }

    if player_wins >= 3 {
        vga::with_writer(|w| {
            w.set_color(Color::LightGreen, Color::Black);
            w.put_str("  *** You win the match! ***\n");
            w.set_color(Color::White, Color::Black);
        });
    } else {
        println!("  CPU wins the match. Try again!");
    }
    println!("  Press any key to continue...");
    wait_key();
}

/// 显示游戏菜单
pub fn menu() {
    println!("");
    vga::with_writer(|w| {
        w.set_color(Color::Pink, Color::Black);
        w.put_str("  === Game Center ===\n");
        w.set_color(Color::White, Color::Black);
    });
    println!("  bomb    Number Bomb - find the bomb safely");
    println!("  guess   Guess the Number - classic 1-100");
    println!("  rps     Rock Paper Scissors - best of 5");
    println!("  back    Return to shell");
}

/// 解析 u32（简单实现）
fn parse_u32(s: &str) -> Option<u32> {
    if s.is_empty() {
        return None;
    }
    let mut val: u32 = 0;
    for b in s.bytes() {
        if !b.is_ascii_digit() {
            return None;
        }
        val = val.checked_mul(10)?.checked_add((b - b'0') as u32)?;
    }
    Some(val)
}
