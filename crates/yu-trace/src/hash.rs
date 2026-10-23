//! 内容身份:本地 SHA-256。
//!
//! 记录封套要求每条 vfs 读取都带**内容身份**,不能只记长度。这里用本地
//! SHA-256(零新依赖,标准 64 位十六进制),避免给工作区引入额外 crate。
//! 它不是安全边界,只用于逐字节对拍与审计;标准测试向量见文件末尾。

const K: [u32; 64] = [
    0x428a_2f98,
    0x7137_4491,
    0xb5c0_fbcf,
    0xe9b5_dba5,
    0x3956_c25b,
    0x59f1_11f1,
    0x923f_82a4,
    0xab1c_5ed5,
    0xd807_aa98,
    0x1283_5b01,
    0x2431_85be,
    0x550c_7dc3,
    0x72be_5d74,
    0x80de_b1fe,
    0x9bdc_06a7,
    0xc19b_f174,
    0xe49b_69c1,
    0xefbe_4786,
    0x0fc1_9dc6,
    0x240c_a1cc,
    0x2de9_2c6f,
    0x4a74_84aa,
    0x5cb0_a9dc,
    0x76f9_88da,
    0x983e_5152,
    0xa831_c66d,
    0xb003_27c8,
    0xbf59_7fc7,
    0xc6e0_0bf3,
    0xd5a7_9147,
    0x06ca_6351,
    0x1429_2967,
    0x27b7_0a85,
    0x2e1b_2138,
    0x4d2c_6dfc,
    0x5338_0d13,
    0x650a_7354,
    0x766a_0abb,
    0x81c2_c92e,
    0x9272_2c85,
    0xa2bf_e8a1,
    0xa81a_664b,
    0xc24b_8b70,
    0xc76c_51a3,
    0xd192_e819,
    0xd699_0624,
    0xf40e_3585,
    0x106a_a070,
    0x19a4_c116,
    0x1e37_6c08,
    0x2748_774c,
    0x34b0_bcb5,
    0x391c_0cb3,
    0x4ed8_aa4a,
    0x5b9c_ca4f,
    0x682e_6ff3,
    0x748f_82ee,
    0x78a5_636f,
    0x84c8_7814,
    0x8cc7_0208,
    0x90be_fffa,
    0xa450_6ceb,
    0xbef9_a3f7,
    0xc671_78f2,
];

const H0: [u32; 8] = [
    0x6a09_e667,
    0xbb67_ae85,
    0x3c6e_f372,
    0xa54f_f53a,
    0x510e_527f,
    0x9b05_688c,
    0x1f83_d9ab,
    0x5be0_cd19,
];

/// 计算 SHA-256 并输出小写十六进制(64 字符)。
pub fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finish()
}

/// 增量 SHA-256。
///
/// 故障快照的 ring buffer 要在**不保留**全部事件的前提下对单 tick 的命令流
/// 做摘要:单 tick 可能产生百万级事件,一次性拼 buffer 再 `sha256_hex`
/// 会把内存放大到不可接受。这里的 `update` 只保留 64 字节块尾,空间恒定。
///
/// 语义与 [`sha256_hex`] 完全一致(同一测试向量)。
#[derive(Clone)]
pub struct Sha256 {
    state: [u32; 8],
    /// 未满一个 64 字节块的前缀。
    buffer: [u8; 64],
    buffer_len: usize,
    /// 已喂入的总字节数(用于长度填充)。
    total_len: u64,
}

impl Default for Sha256 {
    fn default() -> Self {
        Self::new()
    }
}

impl Sha256 {
    pub fn new() -> Self {
        Self {
            state: H0,
            buffer: [0u8; 64],
            buffer_len: 0,
            total_len: 0,
        }
    }

    pub fn update(&mut self, mut data: &[u8]) {
        self.total_len = self.total_len.wrapping_add(data.len() as u64);
        if self.buffer_len > 0 {
            let room = 64 - self.buffer_len;
            let take = room.min(data.len());
            self.buffer[self.buffer_len..self.buffer_len + take].copy_from_slice(&data[..take]);
            self.buffer_len += take;
            data = &data[take..];
            if self.buffer_len == 64 {
                compress(&mut self.state, &self.buffer);
                self.buffer_len = 0;
            }
        }
        let mut chunks = data.chunks_exact(64);
        for block in &mut chunks {
            compress(&mut self.state, block);
        }
        let rest = chunks.remainder();
        if !rest.is_empty() {
            self.buffer[..rest.len()].copy_from_slice(rest);
            self.buffer_len = rest.len();
        }
    }

    /// 结束并输出小写十六进制(64 字符)。
    pub fn finish(mut self) -> String {
        let bit_len = self.total_len.wrapping_mul(8);
        let mut tail = [0u8; 128];
        let mut tail_len = self.buffer_len;
        tail[..tail_len].copy_from_slice(&self.buffer[..tail_len]);
        tail[tail_len] = 0x80;
        tail_len += 1;
        let pad_to = if tail_len <= 56 { 56 } else { 120 };
        tail[pad_to..pad_to + 8].copy_from_slice(&bit_len.to_be_bytes());
        let tail_len = pad_to + 8;
        for block in tail[..tail_len].chunks_exact(64) {
            compress(&mut self.state, block);
        }

        let mut out = String::with_capacity(64);
        for word in self.state {
            out.push_str(&format!("{word:08x}"));
        }
        out
    }
}

fn compress(state: &mut [u32; 8], block: &[u8]) {
    let mut w = [0u32; 64];
    for (i, chunk) in block.chunks_exact(4).enumerate() {
        w[i] = u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
    }
    for i in 16..64 {
        let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
        let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
        w[i] = w[i - 16]
            .wrapping_add(s0)
            .wrapping_add(w[i - 7])
            .wrapping_add(s1);
    }

    let mut a = state[0];
    let mut b = state[1];
    let mut c = state[2];
    let mut d = state[3];
    let mut e = state[4];
    let mut f = state[5];
    let mut g = state[6];
    let mut h = state[7];

    for i in 0..64 {
        let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
        let ch = (e & f) ^ ((!e) & g);
        let t1 = h
            .wrapping_add(s1)
            .wrapping_add(ch)
            .wrapping_add(K[i])
            .wrapping_add(w[i]);
        let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
        let maj = (a & b) ^ (a & c) ^ (b & c);
        let t2 = s0.wrapping_add(maj);

        h = g;
        g = f;
        f = e;
        e = d.wrapping_add(t1);
        d = c;
        c = b;
        b = a;
        a = t1.wrapping_add(t2);
    }

    state[0] = state[0].wrapping_add(a);
    state[1] = state[1].wrapping_add(b);
    state[2] = state[2].wrapping_add(c);
    state[3] = state[3].wrapping_add(d);
    state[4] = state[4].wrapping_add(e);
    state[5] = state[5].wrapping_add(f);
    state[6] = state[6].wrapping_add(g);
    state[7] = state[7].wrapping_add(h);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_vector() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn abc_vector() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn two_block_vector() {
        assert_eq!(
            sha256_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
    }

    #[test]
    fn length_crossing_padding_boundary() {
        // 55/56/64 字节边界必须产生不同且稳定的结果。
        let a = sha256_hex(&[b'a'; 55]);
        let b = sha256_hex(&[b'a'; 56]);
        let c = sha256_hex(&[b'a'; 64]);
        assert_eq!(a.len(), 64);
        assert_ne!(a, b);
        assert_ne!(b, c);
        assert_eq!(a, sha256_hex(&[b'a'; 55]));
    }

    #[test]
    fn incremental_matches_one_shot_for_every_chunking() {
        let data: Vec<u8> = (0..4096u32).map(|i| (i * 37 + 11) as u8).collect();
        let expected = sha256_hex(&data);
        // 覆盖 0 长块、1 字节块、跨越 64/56 边界的块组合。
        for chunk in [1usize, 3, 7, 63, 64, 65, 127, 128, 129, 511] {
            let mut hasher = Sha256::new();
            for piece in data.chunks(chunk) {
                hasher.update(piece);
            }
            // 交错空 update 也不得改变结果。
            hasher.update(&[]);
            assert_eq!(hasher.finish(), expected, "chunk size {chunk}");
        }
        let mut empty = Sha256::new();
        empty.update(&[]);
        assert_eq!(empty.finish(), sha256_hex(b""));
    }
}
