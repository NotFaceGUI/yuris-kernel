//! 最小 WebSocket(RFC 6455)服务端实现。
//!
//! 只覆盖本项目的受控客户端:握手、单帧文本/二进制、ping/pong、close。
//! 不支持分片(消息都很小;收到分片显式报错,不静默拼接)。
//!
//! 握手后必须 `split`:写线程只碰 writer,读线程只碰 reader,避免同一
//! `TcpStream` 上的读写互锁。

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpStream;

use crate::sha::websocket_accept;

/// 客户端消息上限(输入/控制都是几百字节)。
const MAX_PAYLOAD: usize = 1 << 20;

pub enum WsMessage {
    Text(String),
    Binary(Vec<u8>),
    Ping(Vec<u8>),
    Close,
}

pub struct WsReader {
    reader: TcpStream,
}

pub struct WsWriter {
    writer: TcpStream,
}

/// 完成握手并拆分读写半边。
pub fn accept(
    mut stream: TcpStream,
    headers: &BTreeMap<String, String>,
) -> Result<(WsReader, WsWriter), String> {
    // HTTP 阶段的读超时(read_request 为解析请求头设的 10s)对 ws 语义是
    // 错的:呈现端可以长时间不发任何消息(纯看画面),SO_RCVTIMEO 超时会
    // 判死读线程 → 注销客户端 → 写循环 channel 断开 → 整条连接被关,
    // 页面表现为每 ~11s 无谓重连一次。ws 帧边界由 read_message 自身保证。
    stream
        .set_read_timeout(None)
        .map_err(|e| format!("websocket: clear read timeout failed: {e}"))?;
    let key = headers
        .get("sec-websocket-key")
        .ok_or_else(|| "websocket: missing Sec-WebSocket-Key".to_string())?;
    let upgrade = headers
        .get("upgrade")
        .map(|v| v.eq_ignore_ascii_case("websocket"))
        .unwrap_or(false);
    if !upgrade {
        return Err("websocket: missing Upgrade: websocket".to_string());
    }
    let response = format!(
        "HTTP/1.1 101 Switching Protocols\r\n\
         Upgrade: websocket\r\n\
         Connection: Upgrade\r\n\
         Sec-WebSocket-Accept: {}\r\n\r\n",
        websocket_accept(key)
    );
    stream
        .write_all(response.as_bytes())
        .map_err(|e| format!("websocket: handshake write failed: {e}"))?;
    stream
        .flush()
        .map_err(|e| format!("websocket: handshake flush failed: {e}"))?;
    let _ = stream.set_nodelay(true);
    let reader = stream
        .try_clone()
        .map_err(|e| format!("websocket: clone failed: {e}"))?;
    Ok((WsReader { reader }, WsWriter { writer: stream }))
}

impl WsReader {
    /// 读一条消息(ping 以 [`WsMessage::Ping`] 返回,由调用方回 pong;
    /// 分片显式报错)。**读期间不持有 writer 锁**。
    pub fn read_message(&mut self) -> Result<WsMessage, String> {
        loop {
            let mut head = [0u8; 2];
            self.reader
                .read_exact(&mut head)
                .map_err(|e| format!("websocket: read header failed: {e}"))?;
            let fin = head[0] & 0x80 != 0;
            let opcode = head[0] & 0x0f;
            let masked = head[1] & 0x80 != 0;
            let length = match head[1] & 0x7f {
                126 => {
                    let mut buf = [0u8; 2];
                    self.reader
                        .read_exact(&mut buf)
                        .map_err(|e| format!("websocket: read length failed: {e}"))?;
                    u16::from_be_bytes(buf) as usize
                }
                127 => {
                    let mut buf = [0u8; 8];
                    self.reader
                        .read_exact(&mut buf)
                        .map_err(|e| format!("websocket: read length failed: {e}"))?;
                    let value = u64::from_be_bytes(buf);
                    usize::try_from(value)
                        .map_err(|_| "websocket: payload too large".to_string())?
                }
                other => other as usize,
            };
            if length > MAX_PAYLOAD {
                return Err(format!("websocket: payload {length} exceeds {MAX_PAYLOAD}"));
            }
            let mut mask = [0u8; 4];
            if masked {
                self.reader
                    .read_exact(&mut mask)
                    .map_err(|e| format!("websocket: read mask failed: {e}"))?;
            }
            let mut payload = vec![0u8; length];
            self.reader
                .read_exact(&mut payload)
                .map_err(|e| format!("websocket: read payload failed: {e}"))?;
            if masked {
                for (i, byte) in payload.iter_mut().enumerate() {
                    *byte ^= mask[i % 4];
                }
            }
            if !fin {
                return Err("websocket: fragmented messages are not supported".to_string());
            }
            match opcode {
                0x1 => {
                    let text = String::from_utf8(payload)
                        .map_err(|_| "websocket: text frame is not UTF-8".to_string())?;
                    return Ok(WsMessage::Text(text));
                }
                0x2 => return Ok(WsMessage::Binary(payload)),
                0x8 => return Ok(WsMessage::Close),
                0x9 => return Ok(WsMessage::Ping(payload)),
                0xA => {}
                other => return Err(format!("websocket: unsupported opcode {other:#x}")),
            }
        }
    }
}

impl WsWriter {
    pub fn write_binary(&mut self, payload: &[u8]) -> Result<(), String> {
        self.write_frame(0x2, payload)
    }

    pub fn write_text(&mut self, text: &str) -> Result<(), String> {
        self.write_frame(0x1, text.as_bytes())
    }

    pub fn write_pong(&mut self, payload: &[u8]) -> Result<(), String> {
        self.write_frame(0xA, payload)
    }

    fn write_frame(&mut self, opcode: u8, payload: &[u8]) -> Result<(), String> {
        let mut header = Vec::with_capacity(10);
        header.push(0x80 | opcode);
        let len = payload.len();
        if len < 126 {
            header.push(len as u8);
        } else if len <= u16::MAX as usize {
            header.push(126);
            header.extend_from_slice(&(len as u16).to_be_bytes());
        } else {
            header.push(127);
            header.extend_from_slice(&(len as u64).to_be_bytes());
        }
        self.writer
            .write_all(&header)
            .and_then(|_| self.writer.write_all(payload))
            .and_then(|_| self.writer.flush())
            .map_err(|e| format!("websocket: write failed: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::thread;

    #[test]
    fn handshake_and_echo_text() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            // 真实服务端由 HTTP 层先消费请求头;测试里同样要先读掉,
            // 否则 read_message 会把 "GET ..." 当成 WebSocket 帧。
            let mut request = Vec::new();
            let mut chunk = [0u8; 1024];
            loop {
                let read = stream.read(&mut chunk).unwrap();
                request.extend_from_slice(&chunk[..read]);
                if request.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            let mut headers = BTreeMap::new();
            headers.insert(
                "sec-websocket-key".to_string(),
                "dGhlIHNhbXBsZSBub25jZQ==".to_string(),
            );
            headers.insert("upgrade".to_string(), "websocket".to_string());
            let (mut reader, mut writer) = accept(stream, &headers).unwrap();
            match reader.read_message().unwrap() {
                WsMessage::Text(text) => {
                    writer.write_text(&text).unwrap();
                }
                _ => panic!("expected text"),
            }
        });

        let mut client = TcpStream::connect(addr).unwrap();
        client
            .write_all(
                b"GET /ws HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n",
            )
            .unwrap();
        let mut response = Vec::new();
        let mut buf = [0u8; 1];
        loop {
            client.read_exact(&mut buf).unwrap();
            response.push(buf[0]);
            if response.ends_with(b"\r\n\r\n") {
                break;
            }
        }
        let text = String::from_utf8(response).unwrap();
        assert!(text.contains("101 Switching Protocols"));
        assert!(text.contains("s3pPLMBiTxaQ9kYGzzhZRbK+xOo="));

        // 客户端必须掩码。
        let payload = b"hi";
        let mask = [1u8, 2, 3, 4];
        let mut frame = vec![0x81, 0x80 | payload.len() as u8];
        frame.extend_from_slice(&mask);
        frame.extend(payload.iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));
        client.write_all(&frame).unwrap();

        let mut head = [0u8; 2];
        client.read_exact(&mut head).unwrap();
        assert_eq!(head[0], 0x81);
        assert_eq!(head[1], 2);
        let mut echoed = [0u8; 2];
        client.read_exact(&mut echoed).unwrap();
        assert_eq!(&echoed, b"hi");
        server.join().unwrap();
    }
}
