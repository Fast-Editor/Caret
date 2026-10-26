//! Minimal, robust Server-Sent Events parser. Feed raw bytes as they arrive; receive complete events.
//! Handles CRLF/LF, comments, multi-line `data:`, partial frames split across chunks, and UTF-8
//! sequences split across chunk boundaries.

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SseEvent {
    pub event: Option<String>,
    pub data: String,
    pub id: Option<String>,
}

#[derive(Debug, Default)]
pub struct SseParser {
    buf: Vec<u8>,
    event: Option<String>,
    data: Vec<String>,
    id: Option<String>,
    saw_data: bool,
}

impl SseParser {
    pub fn new() -> Self { Self::default() }

    /// Feed a chunk; returns every event completed by this chunk (in order).
    pub fn feed(&mut self, chunk: &[u8]) -> Vec<SseEvent> {
        self.buf.extend_from_slice(chunk);
        let mut out = Vec::new();
        loop {
            let Some(nl) = self.buf.iter().position(|&b| b == b'\n') else { break };
            let line: Vec<u8> = self.buf.drain(..=nl).collect();
            let mut line = &line[..line.len() - 1];
            if line.last() == Some(&b'\r') { line = &line[..line.len() - 1]; }
            if line.is_empty() {
                if let Some(ev) = self.dispatch() { out.push(ev); }
                continue;
            }
            if line[0] == b':' { continue; }
            let text = String::from_utf8_lossy(line);
            let (field, value) = match text.find(':') {
                Some(i) => { let v = &text[i + 1..]; (&text[..i], v.strip_prefix(' ').unwrap_or(v)) }
                None => (&text[..], ""),
            };
            match field {
                "data" => { self.data.push(value.to_string()); self.saw_data = true; }
                "event" => self.event = Some(value.to_string()),
                "id" => self.id = Some(value.to_string()),
                _ => {}
            }
        }
        out
    }

    /// Flush a trailing event that was not terminated by a blank line.
    pub fn finish(&mut self) -> Option<SseEvent> {
        if !self.buf.is_empty() {
            let rest = std::mem::take(&mut self.buf);
            let mut tail = rest;
            tail.push(b'\n');
            let mut evs = self.feed(&tail);
            if let Some(ev) = evs.pop() { return Some(ev); }
        }
        self.dispatch()
    }

    fn dispatch(&mut self) -> Option<SseEvent> {
        if !self.saw_data && self.event.is_none() { self.id = None; return None; }
        let ev = SseEvent { event: self.event.take(), data: self.data.join("\n"), id: self.id.take() };
        self.data.clear();
        self.saw_data = false;
        Some(ev)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_basic_events() {
        let mut p = SseParser::new();
        let evs = p.feed(b"event: message_start\ndata: {\"a\":1}\n\ndata: x\n\n");
        assert_eq!(evs.len(), 2);
        assert_eq!(evs[0].event.as_deref(), Some("message_start"));
        assert_eq!(evs[0].data, "{\"a\":1}");
        assert_eq!(evs[1].event, None);
        assert_eq!(evs[1].data, "x");
    }

    #[test]
    fn handles_split_frames_crlf_comments_and_multiline_data() {
        let mut p = SseParser::new();
        let mut evs = p.feed(b": keep-alive\r\nevent: content_block_delta\r\ndata: {\"te");
        assert!(evs.is_empty());
        evs.extend(p.feed(b"xt\":\"hi\"}\r\n\r\ndata: line1\ndata: line2\n"));
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0].data, "{\"text\":\"hi\"}");
        evs.extend(p.feed(b"\n"));
        assert_eq!(evs[1].data, "line1\nline2");
    }

    #[test]
    fn done_marker_and_finish() {
        let mut p = SseParser::new();
        let evs = p.feed(b"data: [DONE]\n\n");
        assert_eq!(evs[0].data, "[DONE]");
        let mut p = SseParser::new();
        assert!(p.feed(b"data: tail").is_empty());
        assert_eq!(p.finish().unwrap().data, "tail");
    }

    #[test]
    fn utf8_split_across_chunks() {
        let mut p = SseParser::new();
        let s = "data: héllo wörld\n\n".as_bytes();
        let (a, b) = s.split_at(8); // splits inside 'é'
        assert!(p.feed(a).is_empty());
        let evs = p.feed(b);
        assert_eq!(evs[0].data, "héllo wörld");
    }
}
