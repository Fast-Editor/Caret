//! The relay hub: routes JSON frames between remote browser clients and the single desktop executor.
//!
//! Protocol (text frames):
//! * client → server `{ t: "invoke", id, cmd, args, options? }` → executor `{ t: "invoke", client, id, cmd, args, options }`
//! * executor → server `{ t: "reply", client, id, ok, value | error }` → client `{ t: "reply", id, ok, value | error }`
//! * executor → server `{ t: "cb", client, callback, payload }` → client `{ t: "cb", callback, payload }`
//! * client disconnect → executor `{ t: "client_gone", client }`
//! * no executor → the client gets `{ t: "reply", id, ok: false, error: "Caret desktop is not connected" }`
//!
//! The hub is transport agnostic (senders are `mpsc` channels of serialized frames) so it can be unit tested
//! without sockets; `server.rs` plugs WebSockets into it.
use dashmap::DashMap;
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc::UnboundedSender;

pub const NO_EXECUTOR: &str = "Caret desktop is not connected";

pub type Sender = UnboundedSender<String>;
pub type ChangeHook = Arc<dyn Fn() + Send + Sync>;

struct Client { tx: Sender, device_id: String }

#[derive(Default)]
pub struct Hub {
    executor: Mutex<Option<(u64, Sender)>>,
    clients: DashMap<u64, Client>,
    next_id: AtomicU64,
    on_change: Mutex<Option<ChangeHook>>,
}

impl Hub {
    pub fn new() -> Arc<Self> { Arc::new(Self::default()) }

    /// Called whenever the executor or the client set changes (status broadcasts).
    pub fn set_on_change(&self, f: ChangeHook) { *self.on_change.lock() = Some(f); }
    fn changed(&self) { let hook = self.on_change.lock().clone(); if let Some(h) = hook { h(); } }

    /// Attach the desktop executor, replacing a previous one (its sender is dropped, which ends its loop).
    /// Returns a generation id to pass to [`Hub::detach_executor`].
    pub fn attach_executor(&self, tx: Sender) -> u64 {
        let gen = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        *self.executor.lock() = Some((gen, tx));
        self.changed();
        gen
    }
    /// Detach when still the current executor (a newer one is left alone).
    pub fn detach_executor(&self, gen: u64) {
        let mut g = self.executor.lock();
        if g.as_ref().map(|(x, _)| *x == gen).unwrap_or(false) { *g = None; drop(g); self.changed(); }
    }
    pub fn executor_connected(&self) -> bool { self.executor.lock().is_some() }
    fn executor_sender(&self) -> Option<Sender> { self.executor.lock().as_ref().map(|(_, tx)| tx.clone()) }

    pub fn add_client(&self, tx: Sender, device_id: impl Into<String>) -> u64 {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        self.clients.insert(id, Client { tx, device_id: device_id.into() });
        self.changed();
        id
    }
    pub fn remove_client(&self, id: u64) {
        if self.clients.remove(&id).is_some() {
            if let Some(ex) = self.executor_sender() { let _ = ex.send(json!({ "t": "client_gone", "client": id }).to_string()); }
            self.changed();
        }
    }
    pub fn client_count(&self) -> usize { self.clients.len() }
    pub fn client_device(&self, id: u64) -> Option<String> { self.clients.get(&id).map(|c| c.device_id.clone()) }

    /// Disconnect every client of a device (after revocation). Returns how many were dropped.
    pub fn drop_device(&self, device_id: &str) -> usize {
        let ids: Vec<u64> = self.clients.iter().filter(|c| c.device_id == device_id).map(|c| *c.key()).collect();
        for id in &ids { self.remove_client(*id); }
        ids.len()
    }

    fn send_to_client(&self, id: u64, frame: Value) {
        if let Some(c) = self.clients.get(&id) { let _ = c.tx.send(frame.to_string()); }
    }

    /// A frame from client `client`. Only `invoke` is meaningful; everything else is ignored.
    pub fn on_client_frame(&self, client: u64, text: &str) {
        let Ok(v) = serde_json::from_str::<Value>(text) else { return };
        if v.get("t").and_then(Value::as_str) != Some("invoke") { return; }
        let id = v.get("id").cloned().unwrap_or(Value::Null);
        let cmd = v.get("cmd").and_then(Value::as_str).unwrap_or("").to_string();
        if cmd.is_empty() {
            self.send_to_client(client, json!({ "t": "reply", "id": id, "ok": false, "error": "missing cmd" }));
            return;
        }
        let Some(ex) = self.executor_sender() else {
            self.send_to_client(client, json!({ "t": "reply", "id": id, "ok": false, "error": NO_EXECUTOR }));
            return;
        };
        let forwarded = json!({ "t": "invoke", "client": client, "id": id, "cmd": cmd, "args": v.get("args").cloned().unwrap_or_else(|| json!({})), "options": v.get("options").cloned().unwrap_or(Value::Null) });
        if ex.send(forwarded.to_string()).is_err() {
            self.send_to_client(client, json!({ "t": "reply", "id": id, "ok": false, "error": NO_EXECUTOR }));
        }
    }

    /// A frame from the executor: `reply` and `cb` are routed to the addressed client.
    pub fn on_executor_frame(&self, text: &str) {
        let Ok(v) = serde_json::from_str::<Value>(text) else { return };
        let Some(client) = v.get("client").and_then(Value::as_u64) else { return };
        match v.get("t").and_then(Value::as_str) {
            Some("reply") => {
                let mut out = json!({ "t": "reply", "id": v.get("id").cloned().unwrap_or(Value::Null), "ok": v.get("ok").and_then(Value::as_bool).unwrap_or(false) });
                if let Some(val) = v.get("value") { out["value"] = val.clone(); }
                if let Some(err) = v.get("error") { out["error"] = err.clone(); }
                self.send_to_client(client, out);
            }
            Some("cb") => {
                let out = json!({ "t": "cb", "callback": v.get("callback").cloned().unwrap_or(Value::Null), "payload": v.get("payload").cloned().unwrap_or(Value::Null) });
                self.send_to_client(client, out);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver};

    fn recv(rx: &mut UnboundedReceiver<String>) -> Value { serde_json::from_str(&rx.try_recv().expect("a frame")).unwrap() }

    #[test]
    fn invoke_without_executor_is_answered_locally() {
        let hub = Hub::new();
        let (tx, mut rx) = unbounded_channel();
        let c = hub.add_client(tx, "dev1");
        hub.on_client_frame(c, r#"{"t":"invoke","id":7,"cmd":"app_info","args":{}}"#);
        let r = recv(&mut rx);
        assert_eq!(r, json!({ "t": "reply", "id": 7, "ok": false, "error": NO_EXECUTOR }));
        assert!(!hub.executor_connected());
    }

    #[test]
    fn frames_are_routed_between_client_and_executor() {
        let hub = Hub::new();
        let (etx, mut erx) = unbounded_channel();
        let gen = hub.attach_executor(etx);
        let (tx, mut rx) = unbounded_channel();
        let c = hub.add_client(tx, "dev1");
        // client → executor (client id is stamped by the hub, a spoofed one is ignored)
        hub.on_client_frame(c, r#"{"t":"invoke","id":"a1","client":999,"cmd":"fs_read_file","args":{"path":"/x"},"options":{"headers":{}}}"#);
        let fwd = recv(&mut erx);
        assert_eq!(fwd, json!({ "t": "invoke", "client": c, "id": "a1", "cmd": "fs_read_file", "args": { "path": "/x" }, "options": { "headers": {} } }));
        // executor → client reply
        hub.on_executor_frame(&json!({ "t": "reply", "client": c, "id": "a1", "ok": true, "value": { "content": "hi" } }).to_string());
        assert_eq!(recv(&mut rx), json!({ "t": "reply", "id": "a1", "ok": true, "value": { "content": "hi" } }));
        // error reply keeps the error object
        hub.on_executor_frame(&json!({ "t": "reply", "client": c, "id": "a2", "ok": false, "error": { "code": "io", "message": "nope" } }).to_string());
        assert_eq!(recv(&mut rx), json!({ "t": "reply", "id": "a2", "ok": false, "error": { "code": "io", "message": "nope" } }));
        // callbacks
        hub.on_executor_frame(&json!({ "t": "cb", "client": c, "callback": 42, "payload": { "index": 0, "message": "chunk" } }).to_string());
        assert_eq!(recv(&mut rx), json!({ "t": "cb", "callback": 42, "payload": { "index": 0, "message": "chunk" } }));
        // frames for unknown clients and junk are dropped silently
        hub.on_executor_frame(&json!({ "t": "cb", "client": 12345, "callback": 1, "payload": null }).to_string());
        hub.on_executor_frame("not json");
        hub.on_client_frame(c, "{}");
        hub.on_client_frame(c, r#"{"t":"ping"}"#);
        assert!(rx.try_recv().is_err());
        assert!(erx.try_recv().is_err());
        // client leaves → executor is told
        hub.remove_client(c);
        assert_eq!(recv(&mut erx), json!({ "t": "client_gone", "client": c }));
        assert_eq!(hub.client_count(), 0);
        // detaching a stale generation is a no-op, the current one clears
        hub.detach_executor(gen + 100);
        assert!(hub.executor_connected());
        hub.detach_executor(gen);
        assert!(!hub.executor_connected());
    }

    #[test]
    fn newer_executor_replaces_older_and_change_hook_fires() {
        let hub = Hub::new();
        let count = Arc::new(AtomicU64::new(0));
        let c2 = count.clone();
        hub.set_on_change(Arc::new(move || { c2.fetch_add(1, Ordering::Relaxed); }));
        let (e1, mut r1) = unbounded_channel();
        let g1 = hub.attach_executor(e1);
        let (e2, mut r2) = unbounded_channel();
        let _g2 = hub.attach_executor(e2);
        // old sender was dropped: receiving on r1 ends
        assert!(matches!(r1.try_recv(), Err(tokio::sync::mpsc::error::TryRecvError::Disconnected)));
        hub.detach_executor(g1);
        assert!(hub.executor_connected(), "stale detach must not clear the new executor");
        let (tx, _rx) = unbounded_channel();
        let c = hub.add_client(tx, "d");
        hub.on_client_frame(c, r#"{"t":"invoke","id":1,"cmd":"x"}"#);
        assert_eq!(recv(&mut r2)["cmd"], "x");
        assert_eq!(hub.drop_device("d"), 1);
        assert_eq!(recv(&mut r2)["t"], "client_gone");
        assert_eq!(count.load(Ordering::Relaxed), 4, "attach, attach, add, remove");
    }

    #[test]
    fn missing_cmd_is_rejected() {
        let hub = Hub::new();
        let (etx, mut erx) = unbounded_channel();
        hub.attach_executor(etx);
        let (tx, mut rx) = unbounded_channel();
        let c = hub.add_client(tx, "d");
        hub.on_client_frame(c, r#"{"t":"invoke","id":3}"#);
        assert_eq!(recv(&mut rx)["error"], "missing cmd");
        assert!(erx.try_recv().is_err());
    }
}
