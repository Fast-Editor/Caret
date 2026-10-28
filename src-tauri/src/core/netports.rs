//! Listening TCP ports on this machine (the **Ports** panel): which local servers are up, which process owns them,
//! and whether that process was started from one of Caret's terminals. Pure parsers for the platform tools
//! (`lsof -F`, `ss -ltnp`, `netstat -ano`) are unit-tested; the commands only orchestrate them.
use crate::error::{CaretError, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use tauri::{AppHandle, Manager};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ListeningPort {
    pub port: u16,
    /// Bind address as reported by the OS (`127.0.0.1`, `0.0.0.0`, `::1`, `*`).
    pub address: String,
    pub pid: u32,
    /// Short process name (e.g. `node`, `ollama`).
    pub process: String,
    /// Full command line when known (`ps`), else the short name.
    pub command: String,
    /// The process runs under one of Caret's terminals (or under Caret itself).
    pub owned_by_caret: bool,
    pub protocol: String,
}

fn split_addr(name: &str) -> Option<(String, u16)> {
    // `*:8080`, `127.0.0.1:8080`, `[::1]:8080`, `::1:8080` (ss prints `[::]:8080` / `*:8080`)
    let n = name.trim();
    let (addr, port) = if let Some(rest) = n.strip_prefix('[') {
        let (a, p) = rest.split_once("]:")?; (a.to_string(), p)
    } else {
        let idx = n.rfind(':')?; (n[..idx].to_string(), &n[idx + 1..])
    };
    let port: u16 = port.trim_end_matches(|c: char| !c.is_ascii_digit()).parse().ok()?;
    Some((if addr.is_empty() { "*".into() } else { addr }, port))
}

fn addr_rank(a: &str) -> u8 { if a == "*" || a == "0.0.0.0" || a == "::" { 0 } else if a.contains(':') { 1 } else { 2 } }

/// Keep one entry per (pid, port): prefer a concrete IPv4 address over IPv6 over wildcards.
fn dedupe(mut v: Vec<ListeningPort>) -> Vec<ListeningPort> {
    let mut best: HashMap<(u32, u16), ListeningPort> = HashMap::new();
    for p in v.drain(..) {
        let key = (p.pid, p.port);
        match best.get(&key) { Some(cur) if addr_rank(&cur.address) >= addr_rank(&p.address) => {} _ => { best.insert(key, p); } }
    }
    let mut out: Vec<ListeningPort> = best.into_values().collect();
    out.sort_by(|a, b| a.port.cmp(&b.port).then(a.pid.cmp(&b.pid)));
    out
}

/// Parse `lsof -nP -iTCP -sTCP:LISTEN -F pcn` output: `p<pid>` / `c<command>` / `f<fd>` / `n<addr>:<port>` lines.
pub fn parse_lsof(out: &str) -> Vec<ListeningPort> {
    let mut v = vec![]; let mut pid = 0u32; let mut cmd = String::new();
    for line in out.lines() {
        match line.chars().next() {
            Some('p') => { pid = line[1..].trim().parse().unwrap_or(0); cmd.clear(); }
            Some('c') => cmd = line[1..].trim().to_string(),
            Some('n') => { if let Some((addr, port)) = split_addr(&line[1..]) { if pid != 0 { v.push(ListeningPort { port, address: addr, pid, process: cmd.clone(), command: cmd.clone(), owned_by_caret: false, protocol: "tcp".into() }); } } }
            _ => {}
        }
    }
    dedupe(v)
}

/// Parse `ss -ltnpH` output (Linux fallback):
/// `LISTEN 0 128 127.0.0.1:8081 0.0.0.0:* users:(("node",pid=123,fd=20))`
pub fn parse_ss(out: &str) -> Vec<ListeningPort> {
    let mut v = vec![];
    for line in out.lines() {
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() < 4 { continue; }
        let local = if cols[0] == "LISTEN" { cols.get(3) } else { cols.get(2) };
        let Some((addr, port)) = local.and_then(|l| split_addr(l)) else { continue };
        let users = line.split("users:(").nth(1).unwrap_or("");
        let name = users.split('"').nth(1).unwrap_or("").to_string();
        let pid = users.split("pid=").nth(1).and_then(|s| s.chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse().ok()).unwrap_or(0);
        v.push(ListeningPort { port, address: addr, pid, process: name.clone(), command: name, owned_by_caret: false, protocol: "tcp".into() });
    }
    dedupe(v)
}

/// Parse Windows `netstat -ano -p tcp` (LISTENING rows) with `tasklist /fo csv /nh` names.
pub fn parse_netstat_windows(out: &str, names: &HashMap<u32, String>) -> Vec<ListeningPort> {
    let mut v = vec![];
    for line in out.lines() {
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() < 5 || cols[0] != "TCP" || cols[3] != "LISTENING" { continue; }
        let Some((addr, port)) = split_addr(cols[1]) else { continue };
        let pid: u32 = cols[4].parse().unwrap_or(0);
        let name = names.get(&pid).cloned().unwrap_or_default();
        v.push(ListeningPort { port, address: addr, pid, process: name.clone(), command: name, owned_by_caret: false, protocol: "tcp".into() });
    }
    dedupe(v)
}

/// `tasklist /fo csv /nh` → pid → image name.
pub fn parse_tasklist_csv(out: &str) -> HashMap<u32, String> {
    let mut m = HashMap::new();
    for line in out.lines() {
        let cols: Vec<String> = line.split("\",\"").map(|c| c.trim_matches('"').to_string()).collect();
        if cols.len() >= 2 { if let Ok(pid) = cols[1].parse::<u32>() { m.insert(pid, cols[0].clone()); } }
    }
    m
}

/// `ps -axo pid=,ppid=,command=` → (pid → ppid, pid → command line).
pub fn parse_ps(out: &str) -> (HashMap<u32, u32>, HashMap<u32, String>) {
    let mut tree = HashMap::new(); let mut cmds = HashMap::new();
    for line in out.lines() {
        let mut it = line.split_whitespace();
        let (Some(pid), Some(ppid)) = (it.next().and_then(|s| s.parse::<u32>().ok()), it.next().and_then(|s| s.parse::<u32>().ok())) else { continue };
        let rest = it.collect::<Vec<_>>().join(" ");
        tree.insert(pid, ppid); cmds.insert(pid, rest);
    }
    (tree, cmds)
}

/// True when `pid` is one of `roots` or has one of them among its ancestors.
pub fn is_descendant(pid: u32, roots: &HashSet<u32>, tree: &HashMap<u32, u32>) -> bool {
    let mut cur = pid; let mut hops = 0;
    while hops < 64 {
        if roots.contains(&cur) { return true; }
        match tree.get(&cur) { Some(&pp) if pp != 0 && pp != cur => { cur = pp; hops += 1; } _ => return false }
    }
    false
}

fn run(bin: &str, args: &[&str]) -> Option<String> {
    let out = std::process::Command::new(bin).args(args).stdin(std::process::Stdio::null()).output().ok()?;
    Some(String::from_utf8_lossy(&out.stdout).to_string())
}

fn collect_platform() -> Vec<ListeningPort> {
    #[cfg(target_os = "windows")]
    {
        let names = run("tasklist", &["/fo", "csv", "/nh"]).map(|o| parse_tasklist_csv(&o)).unwrap_or_default();
        return run("netstat", &["-ano", "-p", "tcp"]).map(|o| parse_netstat_windows(&o, &names)).unwrap_or_default();
    }
    #[cfg(not(target_os = "windows"))]
    {
        if let Some(o) = run("lsof", &["-nP", "-iTCP", "-sTCP:LISTEN", "-F", "pcn"]) { let v = parse_lsof(&o); if !v.is_empty() || cfg!(target_os = "macos") { return v; } }
        run("ss", &["-ltnpH"]).map(|o| parse_ss(&o)).unwrap_or_default()
    }
}

/// Enrich with full command lines and the Caret-ownership flag (one `ps` call).
fn enrich(mut ports: Vec<ListeningPort>, roots: HashSet<u32>) -> Vec<ListeningPort> {
    #[cfg(not(target_os = "windows"))]
    let (tree, cmds) = run("ps", &["-axo", "pid=,ppid=,command="]).map(|o| parse_ps(&o)).unwrap_or_default();
    #[cfg(target_os = "windows")]
    let (tree, cmds): (HashMap<u32, u32>, HashMap<u32, String>) = Default::default();
    for p in ports.iter_mut() {
        if let Some(c) = cmds.get(&p.pid) { if !c.is_empty() { p.command = c.clone(); if p.process.is_empty() { p.process = c.split_whitespace().next().unwrap_or("").rsplit('/').next().unwrap_or("").to_string(); } } }
        p.owned_by_caret = is_descendant(p.pid, &roots, &tree);
    }
    ports
}

fn caret_roots(app: &AppHandle) -> HashSet<u32> {
    let mut roots: HashSet<u32> = HashSet::from([std::process::id()]);
    if let Some(pty) = app.try_state::<crate::core::pty::PtyState>() { for e in pty.sessions.iter() { let pid = e.value().info.read().pid; if pid != 0 { roots.insert(pid); } } }
    roots
}

/// Listening TCP ports, sorted by port. Fast path for the panel's 3 s refresh (two process spawns, no network).
#[tauri::command]
pub async fn ports_list(app: AppHandle) -> Result<Vec<ListeningPort>> {
    let roots = caret_roots(&app);
    tokio::task::spawn_blocking(move || enrich(collect_platform(), roots)).await.map_err(|e| CaretError::internal(format!("ports task: {e}")))
}

/// Terminate the process that owns a port (SIGTERM, then SIGKILL after 1.5 s if still alive). Refuses Caret itself.
#[tauri::command]
pub async fn ports_kill(pid: u32) -> Result<()> {
    if pid == 0 || pid == std::process::id() || pid == 1 { return Err(CaretError::invalid("refusing to kill that process")); }
    tokio::task::spawn_blocking(move || {
        #[cfg(not(target_os = "windows"))]
        {
            let _ = std::process::Command::new("kill").args(["-TERM", &pid.to_string()]).status();
            std::thread::sleep(std::time::Duration::from_millis(1500));
            let alive = std::process::Command::new("kill").args(["-0", &pid.to_string()]).status().map(|s| s.success()).unwrap_or(false);
            if alive { let _ = std::process::Command::new("kill").args(["-KILL", &pid.to_string()]).status(); }
        }
        #[cfg(target_os = "windows")]
        { let _ = std::process::Command::new("taskkill").args(["/PID", &pid.to_string(), "/T", "/F"]).status(); }
    }).await.map_err(|e| CaretError::internal(format!("kill task: {e}")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    const LSOF: &str = "p880\ncrapportd\nf4\nn*:65110\nf11\nn*:65110\nf20\nn*:59239\np1239\ncollama\nf3\nn127.0.0.1:11434\nf5\nn[::1]:11434\np4242\ncnode\nf21\nn*:3000\nf22\nn127.0.0.1:3000\n";

    #[test]
    fn lsof_parser_groups_by_pid_dedupes_and_prefers_concrete_addresses() {
        let v = parse_lsof(LSOF);
        let ports: Vec<(u16, u32, &str, &str)> = v.iter().map(|p| (p.port, p.pid, p.process.as_str(), p.address.as_str())).collect();
        assert_eq!(ports, vec![(3000, 4242, "node", "127.0.0.1"), (11434, 1239, "ollama", "127.0.0.1"), (59239, 880, "rapportd", "*"), (65110, 880, "rapportd", "*")]);
        assert!(v.iter().all(|p| p.protocol == "tcp" && !p.owned_by_caret));
    }

    #[test]
    fn ss_parser_reads_name_pid_and_ipv6() {
        let out = "LISTEN 0 128 127.0.0.1:8081 0.0.0.0:* users:((\"node\",pid=123,fd=20))\nLISTEN 0 4096 [::]:22 [::]:* users:((\"sshd\",pid=7,fd=3),(\"sshd\",pid=1,fd=3))\n";
        let v = parse_ss(out);
        assert_eq!(v.len(), 2);
        assert_eq!((v[0].port, v[0].pid, v[0].process.as_str()), (22, 7, "sshd"));
        assert_eq!((v[1].port, v[1].pid, v[1].address.as_str()), (8081, 123, "127.0.0.1"));
    }

    #[test]
    fn netstat_windows_parser_uses_tasklist_names() {
        let names = parse_tasklist_csv("\"node.exe\",\"4242\",\"Console\",\"1\",\"50,000 K\"\n\"svchost.exe\",\"900\",\"Services\",\"0\",\"10,000 K\"\n");
        let v = parse_netstat_windows("  Proto  Local Address  Foreign Address  State  PID\n  TCP    0.0.0.0:135    0.0.0.0:0    LISTENING    900\n  TCP    127.0.0.1:3000    0.0.0.0:0    LISTENING    4242\n  TCP    [::]:135    [::]:0    LISTENING    900\n  TCP    10.0.0.5:52000    1.2.3.4:443    ESTABLISHED    4242\n", &names);
        assert_eq!(v.iter().map(|p| (p.port, p.process.as_str())).collect::<Vec<_>>(), vec![(135, "svchost.exe"), (3000, "node.exe")]);
    }

    #[test]
    fn descendants_follow_ps_tree_up_to_a_caret_root() {
        let (tree, cmds) = parse_ps("    1     0 /sbin/launchd\n  500     1 Caret\n  600   500 /bin/zsh -l\n  700   600 node server.js --port 3000\n  800     1 ollama serve\n");
        let roots = HashSet::from([500u32]);
        assert!(is_descendant(700, &roots, &tree)); assert!(is_descendant(600, &roots, &tree)); assert!(!is_descendant(800, &roots, &tree)); assert!(!is_descendant(9999, &roots, &tree));
        assert_eq!(cmds[&700], "node server.js --port 3000");
    }

    #[test]
    fn split_addr_handles_wildcards_and_ipv6() {
        assert_eq!(split_addr("*:8080"), Some(("*".into(), 8080)));
        assert_eq!(split_addr("[::1]:8081"), Some(("::1".into(), 8081)));
        assert_eq!(split_addr("0.0.0.0:443"), Some(("0.0.0.0".into(), 443)));
        assert_eq!(split_addr("nonsense"), None);
    }
}
