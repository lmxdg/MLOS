//! 伪随机数生成器（xorshift64*，以 RDTSC 为种子）

use crate::io::rdtsc;

pub struct Rng {
    state: u64,
}

impl Rng {
    pub fn new() -> Self {
        unsafe {
            let seed = rdtsc() ^ 0x9E37_79B9_7F4A_7C15;
            let mut r = Self {
                state: if seed == 0 { 0x1234_5678_9ABC_DEF0 } else { seed },
            };
            // 预热几次，让低位充分混合
            for _ in 0..10 {
                r.next_u64();
            }
            r
        }
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// 生成 [min, max] 闭区间内的随机数
    pub fn range(&mut self, min: u64, max: u64) -> u64 {
        if max <= min {
            return min;
        }
        min + self.next_u64() % (max - min + 1)
    }
}
