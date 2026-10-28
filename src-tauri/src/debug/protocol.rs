//! Debug Adapter Protocol wire format: `Content-Length: N\r\n\r\n{json}` frames and the three message kinds.
use serde_json::{json, Value};

/// Encode one DAP message (already a JSON object) into a framed byte buffer.
pub fn encode(msg: &Value) -> Vec<u8> {
    let body = serde_json::to_vec(msg).unwrap_or_default();
    let mut out = format!("Content-Length: {}\r\n\r\n", body.len()).into_bytes();
    out.extend_from_slice(&body);
    out
}

pub fn request(seq: i64, command: &str, arguments: Value) -> Value {
    let mut m = json!({ "seq": seq, "type": "request", "command": command });
    if !arguments.is_null() { m["arguments"] = arguments; }
    m
}

pub fn response(seq: i64, request_seq: i64, command: &str, success: bool, body: Value, message: Option<&str>) -> Value {
    let mut m = json!({ "seq": seq, "type": "response", "request_seq": request_seq, "command": command, "success": success });
    if !body.is_null() { m["body"] = body; }
    if let Some(msg) = message { m["message"] = Value::String(msg.to_string()); }
    m
}

/// Incremental frame parser: feed bytes, pull complete JSON messages. Tolerates extra headers and `\n`-only separators.
#[derive(Default)]
pub struct FrameParser { buf: Vec<u8> }

impl FrameParser {
    pub fn new() -> Self { Self::default() }
    pub fn feed(&mut self, bytes: &[u8]) { self.buf.extend_from_slice(bytes); }
    /// Next complete message, if any. Malformed frames are skipped (the parser resyncs on the next header).
    pub fn next(&mut self) -> Option<Value> {
        loop {
            let (hdr_end, sep_len) = match find_header_end(&self.buf) { Some(x) => x, None => return None };
            let header = String::from_utf8_lossy(&self.buf[..hdr_end]).to_string();
            let len = header.lines().find_map(|l| {
                let (k, v) = l.split_once(':')?;
                if k.trim().eq_ignore_ascii_case("content-length") { v.trim().parse::<usize>().ok() } else { None }
            });
            let Some(len) = len else { self.buf.drain(..hdr_end + sep_len); continue; };
            let start = hdr_end + sep_len;
            if self.buf.len() < start + len { return None; }
            let body = self.buf[start..start + len].to_vec();
            self.buf.drain(..start + len);
            match serde_json::from_slice::<Value>(&body) { Ok(v) => return Some(v), Err(_) => continue }
        }
    }
}

fn find_header_end(buf: &[u8]) -> Option<(usize, usize)> {
    if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") { return Some((i, 4)); }
    if let Some(i) = buf.windows(2).position(|w| w == b"\n\n") { return Some((i, 2)); }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn encodes_with_content_length() {
        let m = request(1, "initialize", json!({"clientID": "caret"}));
        let bytes = encode(&m);
        let s = String::from_utf8(bytes.clone()).unwrap();
        assert!(s.starts_with("Content-Length: "));
        let body = s.split("\r\n\r\n").nth(1).unwrap();
        assert_eq!(body.len().to_string(), s.trim_start_matches("Content-Length: ").split("\r\n").next().unwrap());
        assert_eq!(serde_json::from_str::<Value>(body).unwrap()["command"], "initialize");
    }
    #[test]
    fn parses_split_and_concatenated_frames() {
        let a = encode(&request(1, "a", Value::Null)); let b = encode(&request(2, "b", json!({"x": 1})));
        let mut p = FrameParser::new();
        let all: Vec<u8> = a.iter().chain(b.iter()).cloned().collect();
        p.feed(&all[..5]); assert!(p.next().is_none());
        p.feed(&all[5..all.len() - 3]); assert_eq!(p.next().unwrap()["command"], "a"); assert!(p.next().is_none());
        p.feed(&all[all.len() - 3..]); assert_eq!(p.next().unwrap()["seq"], 2); assert!(p.next().is_none());
    }
    #[test]
    fn skips_garbage_and_extra_headers() {
        let mut p = FrameParser::new();
        p.feed(b"Content-Type: application/json\r\nContent-Length: 15\r\n\r\n{\"seq\":3,\"t\":1}");
        assert_eq!(p.next().unwrap()["seq"], 3);
        p.feed(b"X-Bogus: 1\r\n\r\nContent-Length: 2\r\n\r\n{}");
        assert_eq!(p.next().unwrap(), json!({}));
    }
}
