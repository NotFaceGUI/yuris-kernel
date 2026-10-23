//! MT19937(引擎 `MATH[RND]` 的随机源;DIV-RT-014)。
//!
//! # 证据
//!
//! - 引擎初始化 `0x465f64`:`mt[0] = seed`,随后 624 项递推常数
//!   `0x6c078965`(MT19937 `init_genrand` 的标准常数);
//! - 种子来自 `GetTickCount`(`0x461f98` 调 `[0x58c408]` 后立即调
//!   `0x465f64`)—— 非确定,故内核必须注入种子(A3 确定性);
//! - `MATH[RND]`(`0x448571..0x4485b7`):
//!   `SET`/`SET2` 取 64 位整数,若 `min > max` 交换;
//!   `r = rand32 % (max - min + 1)`;结果 `min + r`。
//!   手册 `math/math.html` 例 17:`MATH[LET=@A RND=1 SET=1 SET2=9]`
//!   = 1..9 的随机整数。
//!
//! 默认种子 = 5489(MT19937 参考实现的缺省种子);要逐位复现官方 trace
//! 必须注入同一种子(官方 trace 尚未采集,见 DIV-RT-014)。

/// 标准 MT19937。
#[derive(Clone, Debug)]
pub struct Mt19937 {
    mt: [u32; 624],
    index: usize,
}

impl Mt19937 {
    pub fn new(seed: u32) -> Self {
        let mut mt = [0u32; 624];
        mt[0] = seed;
        for i in 1..624 {
            mt[i] = 1_812_433_253u32
                .wrapping_mul(mt[i - 1] ^ (mt[i - 1] >> 30))
                .wrapping_add(i as u32);
        }
        Self { mt, index: 624 }
    }

    /// 下一个 32 位随机数(标准 MT19937 `genrand_int32`)。
    pub fn next_u32(&mut self) -> u32 {
        const N: usize = 624;
        const M: usize = 397;
        const MATRIX_A: u32 = 0x9908_b0df;
        const UPPER: u32 = 0x8000_0000;
        const LOWER: u32 = 0x7fff_ffff;

        if self.index >= N {
            for kk in 0..N {
                let y = (self.mt[kk] & UPPER) | (self.mt[(kk + 1) % N] & LOWER);
                let mut v = self.mt[(kk + M) % N] ^ (y >> 1);
                if y & 1 != 0 {
                    v ^= MATRIX_A;
                }
                self.mt[kk] = v;
            }
            self.index = 0;
        }
        let mut y = self.mt[self.index];
        self.index += 1;
        y ^= y >> 11;
        y ^= (y << 7) & 0x9d2c_5680;
        y ^= (y << 15) & 0xefc6_0000;
        y ^= y >> 18;
        y
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reference_sequence_seed_5489() {
        // MT19937 参考实现的前三个输出(seed=5489)。
        let mut r = Mt19937::new(5489);
        assert_eq!(r.next_u32(), 3_499_211_612);
        assert_eq!(r.next_u32(), 581_869_302);
        assert_eq!(r.next_u32(), 3_890_346_734);
    }
}
