//! 最小 WebSocket 客户端(bench / HMR 冒烟测试用)。
//!
//! 只覆盖本项目服务端的受控协议:握手、文本/二进制单帧。**不用于产品**。

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

pub struct WsClient {
    stream: TcpStream,
    mask_seed: u32,
}

pub enum Message {
    Text(String),
    Binary(Vec<u8>),
    Close,
}

impl WsClient {
    pub fn connect(port: u16) -> Result<Self, String> {
        let addr = SocketAddr::from(([127, 0, 0, 1], port));
        let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(2))
            .map_err(|e| format!("ws client: connect failed: {e}"))?;
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .map_err(|e| format!("ws client: timeout setup failed: {e}"))?;
        let key = "dGhlIHNhbXBsZSBub25jZQ==";
        let request = format!(
            "GET /ws HTTP/1.1\r\nHost: 127.0.0.1\r\nUpgrade: websocket\r\n\
             Connection: Upgrade\r\nSec-WebSocket-Key: {key}\r\n\
             Sec-WebSocket-Version: 13\r\n\r\n"
        );
        stream
            .write_all(request.as_bytes())
            .map_err(|e| format!("ws client: handshake write failed: {e}"))?;
        let mut response = Vec::new();
        let mut byte = [0u8; 1];
        loop {
            stream
                .read_exact(&mut byte)
                .map_err(|e| format!("ws client: handshake read failed: {e}"))?;
            response.push(byte[0]);
            if response.ends_with(b"\r\n\r\n") {
                break;
            }
            if response.len() > 8192 {
                return Err("ws client: handshake response too large".to_string());
            }
        }
        let text = String::from_utf8_lossy(&response);
        if !text.starts_with("HTTP/1.1 101") {
            return Err(format!(
                "ws client: expected 101, got {}",
                text.lines().next().unwrap_or("")
            ));
        }
        Ok(Self {
            stream,
            mask_seed: 0x9e37_79b9,
        })
    }

    pub fn set_timeout(&self, timeout: Duration) -> Result<(), String> {
        self.stream
            .set_read_timeout(Some(timeout))
            .map_err(|e| format!("ws client: timeout setup failed: {e}"))
    }

    pub fn read_message(&mut self) -> Result<Message, String> {
        loop {
            let mut head = [0u8; 2];
            self.stream
                .read_exact(&mut head)
                .map_err(|e| format!("ws client: read header failed: {e}"))?;
            let opcode = head[0] & 0x0f;
            let masked = head[1] & 0x80 != 0;
            let length = match head[1] & 0x7f {
                126 => {
                    let mut buf = [0u8; 2];
                    self.stream
                        .read_exact(&mut buf)
                        .map_err(|e| format!("ws client: read length failed: {e}"))?;
                    u16::from_be_bytes(buf) as usize
                }
                127 => {
                    let mut buf = [0u8; 8];
                    self.stream
                        .read_exact(&mut buf)
                        .map_err(|e| format!("ws client: read length failed: {e}"))?;
                    usize::try_from(u64::from_be_bytes(buf))
                        .map_err(|_| "ws client: payload too large".to_string())?
                }
                other => other as usize,
            };
            if length > 32 * 1024 * 1024 {
                return Err(format!("ws client: payload {length} too large"));
            }
            let mut mask = [0u8; 4];
            if masked {
                self.stream
                    .read_exact(&mut mask)
                    .map_err(|e| format!("ws client: read mask failed: {e}"))?;
            }
            let mut payload = vec![0u8; length];
            self.stream
                .read_exact(&mut payload)
                .map_err(|e| format!("ws client: read payload failed: {e}"))?;
            if masked {
                for (i, byte) in payload.iter_mut().enumerate() {
                    *byte ^= mask[i % 4];
                }
            }
            match opcode {
                0x1 => {
                    return Ok(Message::Text(
                        String::from_utf8_lossy(&payload).to_string(),
                    ))
                }
                0x2 => return Ok(Message::Binary(payload)),
                0x8 => return Ok(Message::Close),
                0x9 => {
                    self.write_frame(0xA, &payload)?;
                }
                0xA => {}
                _ => return Err(format!("ws client: unsupported opcode {opcode:#x}")),
            }
        }
    }

    pub fn send_text(&mut self, text: &str) -> Result<(), String> {
        self.write_frame(0x1, text.as_bytes())
    }

    fn write_frame(&mut self, opcode: u8, payload: &[u8]) -> Result<(), String> {
        self.mask_seed = self.mask_seed.wrapping_mul(1664525).wrapping_add(1013904223);
        let mask = self.mask_seed.to_be_bytes();
        let mut frame = Vec::with_capacity(payload.len() + 14);
        frame.push(0x80 | opcode);
        let len = payload.len();
        if len < 126 {
            frame.push(0x80 | len as u8);
        } else if len <= u16::MAX as usize {
            frame.push(0x80 | 126);
            frame.extend_from_slice(&(len as u16).to_be_bytes());
        } else {
            frame.push(0x80 | 127);
            frame.extend_from_slice(&(len as u64).to_be_bytes());
        }
        frame.extend_from_slice(&mask);
        frame.extend(
            payload
                .iter()
                .enumerate()
                .map(|(i, byte)| byte ^ mask[i % 4]),
        );
        self.stream
            .write_all(&frame)
            .and_then(|_| self.stream.flush())
            .map_err(|e| format!("ws client: write failed: {e}"))
    }
}
