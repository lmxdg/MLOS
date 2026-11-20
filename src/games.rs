//! 终端小游戏集合：数字炸弹、猜数字、石头剪刀布（中英双语）

use crate::input::{read_line, wait_key};
use crate::rng::Rng;
use core::fmt::Write;
use crate::{println, print, tr, tprintln, tprint};
use crate::vga::{self, Color};

/// 数字炸弹：1~100 中有一个"炸弹"数字，玩家逐步逼近，踩中即失败。
pub fn number_bomb() {
    println!("");
    vga::with_writer(|w| {
        w.set_color(Color::LightRed, Color::Black);
        w.put_str(tr!("  === Number Bomb ===\n", "  === 数字炸弹 ===\n"));
        w.set_color(Color::White, Color::Black);
    });
    tprintln!("  A bomb is hidden in [1,100]. Enter numbers to narrow it down.",
              "  炸弹藏在1到100之间，输入数字逐步逼近。");
    tprintln!("  The closer you get, the more dangerous it becomes.",
              "  越接近炸弹越危险。");
    tprintln!("  Type 'q' to quit. Good luck!\n", "  输入q退出，祝你好运！\n");

    let mut rng = Rng::new();
    let bomb = rng.range(1, 100) as u32;
    let mut buf = [0u8; 32];
    let mut tries = 0u32;

    loop {
        tprint!("  Try #{} | Guess(1-100): ", "  第{}次 | 猜数(1-100): ", tries + 1);
        let n = read_line(&mut buf, None);
        let input = core::str::from_utf8(&buf[..n]).unwrap_or("").trim();

        if input.eq_ignore_ascii_case("q") || input.eq_ignore_ascii_case("quit") {
            tprintln!("  You chicken out. The bomb was {}.", "  你放弃了，炸弹是{}。", bomb);
            return;
        }

        let guess = match parse_u32(input) {
            Some(v) if v >= 1 && v <= 100 => v,
            _ => {
                tprintln!("  Please enter a number between 1 and 100.",
                          "  请输入1到100之间的数字。");
                continue;
            }
        };
        tries += 1;

        let diff = if guess > bomb { guess - bomb } else { bomb - guess };

        if diff == 0 {
            vga::with_writer(|w| {
                w.set_color(Color::LightRed, Color::Black);
                w.put_str(tr!("  *** BOOM! You hit the bomb! Game over. ***\n",
                              "  *** 砰！你踩到炸弹了！游戏结束。***\n"));
                w.set_color(Color::White, Color::Black);
            });
            tprintln!("  The bomb was {} and it took you {} tries.",
                      "  炸弹是{}，你用了{}次。", bomb, tries);
            tprintln!("  Press any key to continue...", "  按任意键继续...");
            wait_key();
            return;
        }

        let danger = if diff >= 40 { 0 } else if diff >= 25 { 1 }
                     else if diff >= 12 { 2 } else if diff >= 5 { 3 } else { 4 };

        let (en, zh, color) = match danger {
            0 => ("SAFE ...  (far away)", "安全...(远)", Color::LightGreen),
            1 => ("Warm ...", "温暖...", Color::Green),
            2 => ("Getting warm ...", "越来越热...", Color::Yellow),
            3 => ("Hot! ...", "很烫！...", Color::LightRed),
            _ => ("VERY HOT! ...", "非常烫！...", Color::Red),
        };
        vga::with_writer(|w| {
            w.set_color(color, Color::Black);
            tprintln!("  {} [distance {}]", "  {} [距离{}]", tr!(en, zh), diff);
            w.set_color(Color::White, Color::Black);
        });
    }
}

/// 猜数字：经典 1~100 猜数字，提示大/小
pub fn guess_number() {
    println!("");
    vga::with_writer(|w| {
        w.set_color(Color::LightCyan, Color::Black);
        w.put_str(tr!("  === Guess the Number ===\n", "  === 猜数字 ===\n"));
        w.set_color(Color::White, Color::Black);
    });
    tprintln!("  I'm thinking of a number between 1 and 100.",
              "  我心里想了一个1到100的数字。");
    tprintln!("  Type 'q' to quit.\n", "  输入q退出。\n");

    let mut rng = Rng::new();
    let secret = rng.range(1, 100) as u32;
    let mut buf = [0u8; 32];
    let mut tries = 0u32;

    loop {
        tprint!("  Your guess: ", "  你的猜测: ");
        let n = read_line(&mut buf, None);
        let input = core::str::from_utf8(&buf[..n]).unwrap_or("").trim();

        if input.eq_ignore_ascii_case("q") || input.eq_ignore_ascii_case("quit") {
            tprintln!("  It was {}. Maybe next time!", "  是{}，下次加油！", secret);
            return;
        }

        let guess = match parse_u32(input) {
            Some(v) if v >= 1 && v <= 100 => v,
            _ => {
                tprintln!("  Enter a number between 1 and 100.", "  请输入1到100的数字。");
                continue;
            }
        };
        tries += 1;

        if guess < secret {
            tprintln!("  Too small! Try higher.", "  太小了！再大点。");
        } else if guess > secret {
            tprintln!("  Too big! Try lower.", "  太大了！再小点。");
        } else {
            vga::with_writer(|w| {
                w.set_color(Color::LightGreen, Color::Black);
                w.put_str(tr!("  Correct! You got it!\n", "  *** 猜对了！***\n"));
                w.set_color(Color::White, Color::Black);
            });
            tprintln!("  The number was {} and you found it in {} tries.",
                      "  数字是{}，你用了{}次猜中。", secret, tries);
            tprintln!("  Press any key to continue...", "  按任意键继续...");
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
        w.put_str(tr!("  === Rock Paper Scissors ===\n", "  === 石头剪刀布 ===\n"));
        w.set_color(Color::White, Color::Black);
    });
    tprintln!("  Type r(rock), p(paper), s(scissors), or q to quit.",
              "  输入r(石头)p(布)s(剪刀)q(退出)。");
    tprintln!("  First to 3 wins!\n", "  先赢三局者获胜！\n");

    let mut rng = Rng::new();
    let mut buf = [0u8; 32];
    let mut player_wins = 0u32;
    let mut cpu_wins = 0u32;

    while player_wins < 3 && cpu_wins < 3 {
        tprint!("  Your move [r/p/s/q]: ", "  你的出招[r/p/s/q]: ");
        let n = read_line(&mut buf, None);
        let input = core::str::from_utf8(&buf[..n]).unwrap_or("").trim();
        let first = input.as_bytes().first().copied().unwrap_or(0) as char;

        let player = match first.to_ascii_lowercase() {
            'r' => 0,
            'p' => 1,
            's' => 2,
            'q' => {
                tprintln!("  Quit. Score: You {} - {} CPU", "  退出。比分 你{} - {}电脑", player_wins, cpu_wins);
                return;
            }
            _ => {
                tprintln!("  Invalid move. Use r, p, s, or q.", "  无效出招，请用r、p、s或q。");
                continue;
            }
        };

        let cpu = rng.range(0, 3) as usize;
        let en_names = ["rock", "paper", "scissors"];
        let zh_names = ["石头", "布", "剪刀"];
        tprintln!("  You: {}   CPU: {}", "  你: {}   电脑: {}",
                  tr!(en_names[player], zh_names[player]), tr!(en_names[cpu], zh_names[cpu]));

        if player == cpu {
            tprintln!("  -> Draw!", "  -> 平局！");
        } else if cpu == (player + 1) % 3 {
            tprintln!("  -> CPU wins this round!", "  -> 电脑赢这局！");
            cpu_wins += 1;
        } else {
            tprintln!("  -> You win this round!", "  -> 你赢这局！");
            player_wins += 1;
        }
        tprintln!("  Score: You {} - {} CPU\n", "  比分: 你{} - {}电脑\n", player_wins, cpu_wins);
    }

    if player_wins >= 3 {
        vga::with_writer(|w| {
            w.set_color(Color::LightGreen, Color::Black);
            w.put_str(tr!("  *** You win the match! ***\n", "  *** 你赢下比赛！***\n"));
            w.set_color(Color::White, Color::Black);
        });
    } else {
        tprintln!("  CPU wins the match. Try again!", "  电脑赢了比赛，再接再厉！");
    }
    tprintln!("  Press any key to continue...", "  按任意键继续...");
    wait_key();
}

/// 显示游戏菜单
pub fn menu() {
    println!("");
    vga::with_writer(|w| {
        w.set_color(Color::Pink, Color::Black);
        w.put_str(tr!("  === Game Center ===\n", "  === 游戏中心 ===\n"));
        w.set_color(Color::White, Color::Black);
    });
    tprintln!("  bomb    Number Bomb - find the bomb safely", "  bomb    数字炸弹 - 安全逼近炸弹");
    tprintln!("  guess   Guess the Number - classic 1-100", "  guess   猜数字 - 经典1到100");
    tprintln!("  rps     Rock Paper Scissors - best of 5", "  rps     石头剪刀布 - 五局三胜");
    tprintln!("  back    Return to shell", "  back    返回命令行");
}

/// 解析 u32
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
