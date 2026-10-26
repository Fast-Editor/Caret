//! Dependency grounding: every package the change introduces must exist in a registry, be in the lockfile
//! when one is present, not be a near-miss of a popular name (slopsquatting), and have no known OSV advisories.
use super::CheckCtx;
use crate::error::Result;
use crate::verify::scope::{FileClass, Lang, TouchedFile};
use crate::verify::{VerifyCheck, VerifyFinding};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Ecosystem { Npm, Crates, PyPi, Go }
impl Ecosystem {
    pub fn osv_name(self) -> &'static str { match self { Ecosystem::Npm => "npm", Ecosystem::Crates => "crates.io", Ecosystem::PyPi => "PyPI", Ecosystem::Go => "Go" } }
    pub fn label(self) -> &'static str { match self { Ecosystem::Npm => "npm", Ecosystem::Crates => "crates.io", Ecosystem::PyPi => "PyPI", Ecosystem::Go => "Go proxy" } }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DepRef { pub eco: Ecosystem, pub name: String, pub version: Option<String>, pub path: String, pub line: Option<u32>, pub from_manifest: bool }

static RE_JS_IMPORT: Lazy<Regex> = Lazy::new(|| Regex::new(r#"(?:^|[^\w.])(?:import\s+(?:[\w*{}\s,$]+\s+from\s+)?|export\s+(?:[\w*{}\s,$]+\s+from\s+)|require\(\s*|import\(\s*)["']([^"'\n]+)["']"#).unwrap());
static RE_PY_IMPORT: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?m)^\s*(?:from\s+([A-Za-z_][\w]*)[\w.]*\s+import|import\s+([A-Za-z_][\w]*(?:\s*,\s*[A-Za-z_]\w*)*))").unwrap());
static RE_RS_USE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?m)^\s*(?:pub(?:\([^)]*\))?\s+)?use\s+(?:::)?([a-z_][a-z0-9_]*)\s*(?:::|;)").unwrap());
static RE_RS_EXTERN: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?m)^\s*extern\s+crate\s+([a-z_][a-z0-9_]*)").unwrap());
static RE_GO_IMPORT: Lazy<Regex> = Lazy::new(|| Regex::new(r#""((?:github\.com|gitlab\.com|golang\.org|google\.golang\.org|gopkg\.in|go\.uber\.org|k8s\.io|sigs\.k8s\.io|[a-z0-9.-]+\.[a-z]{2,})/[^"\s]+)""#).unwrap());
static RE_CARGO_DEP: Lazy<Regex> = Lazy::new(|| Regex::new(r#"(?m)^\s*([A-Za-z0-9_-]+)\s*=\s*(?:"([^"]*)"|\{[^}]*?(?:version\s*=\s*"([^"]*)")?[^}]*\})"#).unwrap());
static RE_REQ_LINE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?m)^\s*([A-Za-z0-9][A-Za-z0-9._-]*)(?:\[[^\]]*\])?\s*(?:([<>=!~]=?)\s*([\w.*+-]+))?").unwrap());
static RE_PYPROJECT_DEP: Lazy<Regex> = Lazy::new(|| Regex::new(r#""([A-Za-z0-9][A-Za-z0-9._-]*)(?:\[[^\]]*\])?\s*(?:[<>=!~]=?\s*([\w.*+-]+))?[^"]*""#).unwrap());
static RE_GOMOD_REQ: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?m)^\s*(?:require\s+)?([a-z0-9.-]+\.[a-z]{2,}/[^\s]+)\s+(v[\w.+-]+)").unwrap());

const NODE_BUILTINS: &[&str] = &["assert","async_hooks","buffer","child_process","cluster","console","constants","crypto","dgram","diagnostics_channel","dns","domain","events","fs","http","http2","https","inspector","module","net","os","path","perf_hooks","process","punycode","querystring","readline","repl","stream","string_decoder","sys","timers","tls","trace_events","tty","url","util","v8","vm","wasi","worker_threads","zlib","test"];
const PY_STDLIB: &[&str] = &["abc","argparse","array","ast","asyncio","base64","bisect","builtins","bz2","calendar","cmath","collections","concurrent","configparser","contextlib","copy","csv","ctypes","dataclasses","datetime","decimal","difflib","dis","email","enum","errno","faulthandler","fnmatch","fractions","functools","gc","getpass","gettext","glob","graphlib","gzip","hashlib","heapq","hmac","html","http","imaplib","importlib","inspect","io","ipaddress","itertools","json","keyword","linecache","locale","logging","lzma","math","mimetypes","multiprocessing","numbers","operator","os","pathlib","pickle","platform","plistlib","pprint","queue","random","re","sched","secrets","select","selectors","shlex","shutil","signal","socket","socketserver","sqlite3","ssl","stat","statistics","string","struct","subprocess","sys","sysconfig","tarfile","tempfile","textwrap","threading","time","timeit","token","tokenize","tomllib","traceback","types","typing","unicodedata","unittest","urllib","uuid","venv","warnings","weakref","xml","zipfile","zipimport","zlib","__future__","typing_extensions_stub"];
const RUST_BUILTINS: &[&str] = &["std","core","alloc","crate","self","super","proc_macro","test"];
/// Popular names used for near-miss detection (slopsquatting / hallucinated variants).
const POPULAR_NPM: &[&str] = &["react","react-dom","lodash","express","axios","chalk","commander","typescript","vite","next","vue","zod","dayjs","moment","uuid","dotenv","prisma","@prisma/client","tailwindcss","eslint","prettier","jest","vitest","mongoose","pg","mysql2","redis","ioredis","socket.io","ws","cors","body-parser","jsonwebtoken","bcrypt","bcryptjs","passport","openai","@anthropic-ai/sdk","langchain","@langchain/core","cheerio","puppeteer","playwright","sharp","multer","nodemailer","winston","pino","debug","yargs","inquirer","ora","glob","fast-glob","rimraf","fs-extra","esbuild","rollup","webpack","babel","@babel/core","svelte","solid-js","zustand","redux","@reduxjs/toolkit","react-query","@tanstack/react-query","swr","graphql","apollo-server","@apollo/client","stripe","twilio","aws-sdk","@aws-sdk/client-s3","firebase","date-fns","ramda","immer","classnames","clsx","nanoid","semver","minimist","node-fetch","undici","got","superagent","form-data","validator","joi","yup","ajv","marked","markdown-it","highlight.js","prismjs","three","d3","chart.js","leaflet","mapbox-gl","socket.io-client","mqtt","amqplib","kafkajs","bullmq","agenda","node-cron","cron","helmet","compression","morgan","cookie-parser","express-session","connect-redis","sequelize","typeorm","knex","drizzle-orm","better-sqlite3","sqlite3","level","nedb","lowdb"];
const POPULAR_PYPI: &[&str] = &["requests","numpy","pandas","flask","django","fastapi","pydantic","sqlalchemy","httpx","aiohttp","boto3","botocore","pytest","black","ruff","mypy","matplotlib","seaborn","scipy","scikit-learn","sklearn","torch","tensorflow","keras","transformers","openai","anthropic","langchain","llama-index","beautifulsoup4","bs4","lxml","scrapy","selenium","playwright","pillow","opencv-python","pyyaml","toml","tomli","click","typer","rich","tqdm","loguru","structlog","celery","redis","pymongo","psycopg2","psycopg2-binary","psycopg","asyncpg","alembic","uvicorn","gunicorn","starlette","jinja2","markupsafe","cryptography","pyjwt","passlib","bcrypt","python-dotenv","dotenv","pydantic-settings","attrs","cattrs","marshmallow","dateutil","python-dateutil","pytz","arrow","pendulum","orjson","ujson","msgpack","protobuf","grpcio","websockets","paramiko","fabric","ansible","docker","kubernetes","pyarrow","polars","dask","ray","xgboost","lightgbm","catboost","statsmodels","sympy","networkx","nltk","spacy","gensim","sentence-transformers","faiss-cpu","chromadb","qdrant-client","pinecone-client","weaviate-client","tiktoken","streamlit","gradio","dash","plotly","bokeh","altair"];
const POPULAR_CRATES: &[&str] = &["serde","serde_json","tokio","anyhow","thiserror","clap","reqwest","hyper","axum","actix-web","rocket","warp","tracing","log","env_logger","rand","regex","chrono","time","uuid","once_cell","lazy_static","itertools","rayon","crossbeam","parking_lot","dashmap","futures","async-trait","bytes","base64","sha2","hmac","ring","rustls","openssl","sqlx","diesel","sea-orm","rusqlite","redis","mongodb","tonic","prost","bincode","toml","yaml-rust","serde_yaml","walkdir","ignore","glob","notify","tempfile","dirs","which","indicatif","console","dialoguer","colored","owo-colors","nom","pest","syn","quote","proc-macro2","libc","nix","windows","winapi","wasm-bindgen","web-sys","js-sys","tauri","egui","iced","bevy","image","tokio-util","tokio-stream","url","mime","http","tower","tower-http","jsonwebtoken","argon2","bcrypt","blake3","zstd","flate2","tar","zip","csv","polars","ndarray","num","num-traits","petgraph","smallvec","arrayvec","bitflags","strum","derive_more","semver","mimalloc","jemallocator","criterion","proptest","insta","mockall","wiremock"];

fn is_relative(spec: &str) -> bool { spec.starts_with('.') || spec.starts_with('/') || spec.starts_with('~') || spec.starts_with('#') || spec.starts_with("@/") || spec.starts_with("src/") || spec.starts_with("node:") || spec.starts_with("bun:") || spec.starts_with("deno:") || spec.starts_with("https://") || spec.starts_with("npm:") || spec.starts_with("jsr:") || spec.starts_with("virtual:") || spec.starts_with('$') }

/// Reduce a JS import specifier to its package name (`@scope/name` or `name`), dropping subpaths and `?worker` suffixes.
pub fn npm_package_name(spec: &str) -> Option<String> {
    if is_relative(spec) { return None; }
    let spec = spec.split('?').next().unwrap_or(spec);
    let mut parts = spec.split('/');
    let first = parts.next()?;
    let name = if first.starts_with('@') { format!("{}/{}", first, parts.next()?) } else { first.to_string() };
    if name.is_empty() || !name.chars().next().map(|c| c.is_ascii_alphanumeric() || c == '@').unwrap_or(false) { return None; }
    if NODE_BUILTINS.contains(&name.as_str()) { return None; }
    Some(name)
}

/// Collect dependency references from touched files (added lines only for source; full file for manifests).
pub fn extract(files: &[TouchedFile]) -> Vec<DepRef> {
    let mut out: Vec<DepRef> = vec![];
    let mut seen: HashSet<(Ecosystem, String)> = HashSet::new();
    let mut push = |r: DepRef| { if seen.insert((r.eco, r.name.clone())) { out.push(r); } };
    for f in files {
        let Some(after) = f.after.as_deref() else { continue };
        let base = f.path.rsplit('/').next().unwrap_or(&f.path);
        if f.class == FileClass::Manifest {
            match base {
                "package.json" => {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(after) {
                        for key in ["dependencies", "devDependencies", "peerDependencies", "optionalDependencies"] {
                            if let Some(obj) = v[key].as_object() { for (name, ver) in obj { push(DepRef { eco: Ecosystem::Npm, name: name.clone(), version: ver.as_str().map(|s| s.trim_start_matches(['^', '~', '=']).to_string()), path: f.path.clone(), line: line_of(after, &format!("\"{name}\"")), from_manifest: true }); } }
                        }
                    }
                }
                "Cargo.toml" => {
                    let mut in_deps = false;
                    for (i, line) in after.lines().enumerate() {
                        let t = line.trim();
                        if t.starts_with('[') { in_deps = t.contains("dependencies") && !t.starts_with("[dependencies."); if t.starts_with("[dependencies.") || t.starts_with("[dev-dependencies.") || t.starts_with("[build-dependencies.") { let name = t.trim_start_matches('[').trim_end_matches(']').rsplit('.').next().unwrap_or("").to_string(); push(DepRef { eco: Ecosystem::Crates, name, version: None, path: f.path.clone(), line: Some(i as u32 + 1), from_manifest: true }); } continue; }
                        if in_deps { if let Some(c) = RE_CARGO_DEP.captures(line) { let name = c[1].to_string(); if line.contains("path =") || line.contains("git =") || line.contains("workspace = true") { continue; } let version = c.get(2).or(c.get(3)).map(|m| m.as_str().to_string()); push(DepRef { eco: Ecosystem::Crates, name, version, path: f.path.clone(), line: Some(i as u32 + 1), from_manifest: true }); } }
                    }
                }
                "requirements.txt" | "requirements-dev.txt" | "requirements.in" => {
                    for (i, line) in after.lines().enumerate() { let t = line.trim(); if t.is_empty() || t.starts_with('#') || t.starts_with('-') || t.contains("://") || t.starts_with('.') || t.starts_with('/') { continue; } if let Some(c) = RE_REQ_LINE.captures(t) { push(DepRef { eco: Ecosystem::PyPi, name: normalize_pypi(&c[1]), version: if c.get(2).map(|m| m.as_str()) == Some("==") { c.get(3).map(|m| m.as_str().to_string()) } else { None }, path: f.path.clone(), line: Some(i as u32 + 1), from_manifest: true }); } }
                }
                "pyproject.toml" => {
                    let mut in_deps = false;
                    for (i, line) in after.lines().enumerate() {
                        let t = line.trim();
                        if t.starts_with('[') { in_deps = t.contains("dependencies") || t.contains("optional-dependencies") || t.contains("dependency-groups"); continue; }
                        if t.starts_with("dependencies") && t.contains('[') { in_deps = true; }
                        if in_deps { for c in RE_PYPROJECT_DEP.captures_iter(line) { let name = normalize_pypi(&c[1]); if name == "python" { continue; } push(DepRef { eco: Ecosystem::PyPi, name, version: c.get(2).map(|m| m.as_str().to_string()), path: f.path.clone(), line: Some(i as u32 + 1), from_manifest: true }); } }
                    }
                }
                "go.mod" => { for (i, line) in after.lines().enumerate() { if let Some(c) = RE_GOMOD_REQ.captures(line) { if line.contains("// indirect") { continue; } push(DepRef { eco: Ecosystem::Go, name: c[1].to_string(), version: Some(c[2].to_string()), path: f.path.clone(), line: Some(i as u32 + 1), from_manifest: true }); } } }
                _ => {}
            }
            continue;
        }
        if f.class != FileClass::Source && f.class != FileClass::Test { continue; }
        for (ln, text) in &f.added_lines {
            match f.lang {
                Lang::Js | Lang::Ts => { for c in RE_JS_IMPORT.captures_iter(text) { if let Some(name) = npm_package_name(&c[1]) { push(DepRef { eco: Ecosystem::Npm, name, version: None, path: f.path.clone(), line: Some(*ln), from_manifest: false }); } } }
                Lang::Python => { for c in RE_PY_IMPORT.captures_iter(text) { let names: Vec<String> = c.get(1).map(|m| vec![m.as_str().to_string()]).unwrap_or_else(|| c.get(2).map(|m| m.as_str().split(',').map(|s| s.trim().split_whitespace().next().unwrap_or("").to_string()).collect()).unwrap_or_default()); for n in names { if n.is_empty() || PY_STDLIB.contains(&n.as_str()) || n.starts_with('_') { continue; } push(DepRef { eco: Ecosystem::PyPi, name: n.to_lowercase(), version: None, path: f.path.clone(), line: Some(*ln), from_manifest: false }); } } }
                Lang::Rust => { for re in [&*RE_RS_USE, &*RE_RS_EXTERN] { for c in re.captures_iter(text) { let n = c[1].to_string(); if RUST_BUILTINS.contains(&n.as_str()) { continue; } push(DepRef { eco: Ecosystem::Crates, name: n.replace('_', "-"), version: None, path: f.path.clone(), line: Some(*ln), from_manifest: false }); } } }
                Lang::Go => { for c in RE_GO_IMPORT.captures_iter(text) { push(DepRef { eco: Ecosystem::Go, name: go_module_root(&c[1]), version: None, path: f.path.clone(), line: Some(*ln), from_manifest: false }); } }
                _ => {}
            }
        }
    }
    out
}
fn line_of(text: &str, needle: &str) -> Option<u32> { text.lines().position(|l| l.contains(needle)).map(|i| i as u32 + 1) }
pub fn normalize_pypi(n: &str) -> String { n.to_lowercase().replace(['_', '.'], "-") }
fn go_module_root(p: &str) -> String { let parts: Vec<&str> = p.split('/').collect(); if parts.len() >= 3 && (parts[0] == "github.com" || parts[0] == "gitlab.com" || parts[0] == "bitbucket.org") { parts[..3].join("/") } else if parts[0] == "golang.org" && parts.len() >= 3 { parts[..3].join("/") } else { p.to_string() } }

/// Names declared in the workspace's manifests/lockfiles (so imports of already-declared packages aren't re-resolved).
pub struct Declared { pub npm: HashSet<String>, pub crates: HashSet<String>, pub pypi: HashSet<String>, pub go: HashSet<String>, pub lock_npm: Option<HashSet<String>>, pub lock_crates: Option<HashSet<String>>, pub lock_go: Option<HashSet<String>>, pub import_map: HashMap<String, String> }
pub fn declared(root: &std::path::Path) -> Declared {
    let mut d = Declared { npm: HashSet::new(), crates: HashSet::new(), pypi: HashSet::new(), go: HashSet::new(), lock_npm: None, lock_crates: None, lock_go: None, import_map: HashMap::new() };
    if let Ok(s) = std::fs::read_to_string(root.join("package.json")) { if let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) { for key in ["dependencies", "devDependencies", "peerDependencies", "optionalDependencies"] { if let Some(o) = v[key].as_object() { d.npm.extend(o.keys().cloned()); } } if let Some(o) = v["imports"].as_object() { d.npm.extend(o.keys().cloned()); } } }
    if let Ok(s) = std::fs::read_to_string(root.join("package-lock.json")) { if let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) { let mut set = HashSet::new(); if let Some(o) = v["packages"].as_object() { for k in o.keys() { if let Some(i) = k.rfind("node_modules/") { set.insert(k[i + 13..].to_string()); } } } if let Some(o) = v["dependencies"].as_object() { set.extend(o.keys().cloned()); } d.lock_npm = Some(set); } }
    else if let Ok(s) = std::fs::read_to_string(root.join("pnpm-lock.yaml")) { let set: HashSet<String> = s.lines().filter_map(|l| { let t = l.trim_end(); if t.starts_with("  /") || t.starts_with("  '") || (t.starts_with("  ") && !t.starts_with("   ") && t.ends_with(':')) { Some(t.trim().trim_start_matches('/').trim_start_matches('\'').trim_end_matches(':').trim_end_matches('\'').to_string()) } else { None } }).map(|k| { let k = k.trim_start_matches('/').to_string(); if let Some(at) = k[1..].find('@') { k[..at + 1].to_string() } else { k } }).collect(); d.lock_npm = Some(set); }
    else if let Ok(s) = std::fs::read_to_string(root.join("yarn.lock")) { let set: HashSet<String> = s.lines().filter(|l| !l.starts_with(' ') && l.ends_with(':') && !l.starts_with('#')).flat_map(|l| l.trim_end_matches(':').split(", ").map(|spec| { let spec = spec.trim_matches('"'); if let Some(at) = spec[1..].find('@') { spec[..at + 1].to_string() } else { spec.to_string() } }).collect::<Vec<_>>()).collect(); d.lock_npm = Some(set); }
    else if root.join("bun.lockb").exists() || root.join("bun.lock").exists() { d.lock_npm = None; }
    if let Ok(s) = std::fs::read_to_string(root.join("Cargo.toml")) { for cap in RE_CARGO_DEP.captures_iter(&s) { d.crates.insert(cap[1].to_string()); d.crates.insert(cap[1].replace('_', "-")); } }
    if let Ok(s) = std::fs::read_to_string(root.join("Cargo.lock")) { d.lock_crates = Some(s.lines().filter_map(|l| l.strip_prefix("name = \"")).map(|n| n.trim_end_matches('"').to_string()).collect()); }
    for req in ["requirements.txt", "requirements-dev.txt", "pyproject.toml"] { if let Ok(s) = std::fs::read_to_string(root.join(req)) { for c in RE_REQ_LINE.captures_iter(&s) { d.pypi.insert(normalize_pypi(&c[1])); } for c in RE_PYPROJECT_DEP.captures_iter(&s) { d.pypi.insert(normalize_pypi(&c[1])); } } }
    if let Ok(s) = std::fs::read_to_string(root.join("go.mod")) { for c in RE_GOMOD_REQ.captures_iter(&s) { d.go.insert(c[1].to_string()); } }
    if let Ok(s) = std::fs::read_to_string(root.join("go.sum")) { d.lock_go = Some(s.lines().filter_map(|l| l.split_whitespace().next()).map(|s| s.to_string()).collect()); }
    // Common PyPI import-name → distribution-name mappings.
    for (imp, dist) in [("bs4", "beautifulsoup4"), ("yaml", "pyyaml"), ("cv2", "opencv-python"), ("PIL", "pillow"), ("sklearn", "scikit-learn"), ("dotenv", "python-dotenv"), ("jwt", "pyjwt"), ("dateutil", "python-dateutil"), ("attr", "attrs"), ("google", "google-api-core"), ("Crypto", "pycryptodome"), ("serial", "pyserial"), ("psycopg2", "psycopg2-binary"), ("OpenSSL", "pyopenssl"), ("magic", "python-magic"), ("docx", "python-docx"), ("pptx", "python-pptx"), ("fitz", "pymupdf"), ("usb", "pyusb"), ("zmq", "pyzmq"), ("gi", "pygobject"), ("wx", "wxpython"), ("markdown", "markdown"), ("mx", "mxnet"), ("sqlalchemy", "sqlalchemy")] { d.import_map.insert(imp.to_lowercase(), dist.to_string()); }
    d
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct Cache { #[serde(default)] exists: BTreeMap<String, (bool, u64)>, #[serde(default)] osv: BTreeMap<String, (Vec<String>, u64)> }
fn cache_path() -> std::path::PathBuf { crate::paths::sub_dir("verify").join("registry-cache.json") }
fn now() -> u64 { std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) }
fn load_cache() -> Cache { std::fs::read_to_string(cache_path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default() }
fn save_cache(c: &Cache) { if let Ok(s) = serde_json::to_string(c) { let _ = std::fs::write(cache_path(), s); } }
const TTL_EXISTS: u64 = 7 * 86400; const TTL_MISSING: u64 = 3600; const TTL_OSV: u64 = 86400;

fn registry_url(eco: Ecosystem, name: &str) -> String {
    match eco {
        Ecosystem::Npm => format!("https://registry.npmjs.org/{}", name.replace('/', "%2F")),
        Ecosystem::Crates => format!("https://crates.io/api/v1/crates/{name}"),
        Ecosystem::PyPi => format!("https://pypi.org/pypi/{name}/json"),
        Ecosystem::Go => format!("https://proxy.golang.org/{}/@v/list", name.to_lowercase()),
    }
}

async fn exists(client: &reqwest::Client, cache: &mut Cache, eco: Ecosystem, name: &str) -> Option<bool> {
    let key = format!("{}:{}", eco.osv_name(), name);
    if let Some((ok, ts)) = cache.exists.get(&key) { let ttl = if *ok { TTL_EXISTS } else { TTL_MISSING }; if now().saturating_sub(*ts) < ttl { return Some(*ok); } }
    let mut req = client.head(registry_url(eco, name));
    if eco == Ecosystem::Crates { req = client.get(registry_url(eco, name)).header("User-Agent", "caret-verify (https://github.com/caret-editor)"); }
    if eco == Ecosystem::Go { req = client.get(registry_url(eco, name)); }
    let resp = req.send().await.ok()?;
    let status = resp.status().as_u16();
    let ok = match status { 200..=299 => true, 404 | 410 => false, 405 if eco == Ecosystem::Npm => { client.get(registry_url(eco, name)).send().await.ok()?.status().is_success() } _ => return None };
    cache.exists.insert(key, (ok, now()));
    Some(ok)
}

#[derive(Serialize)] struct OsvQuery<'a> { package: OsvPkg<'a>, #[serde(skip_serializing_if = "Option::is_none")] version: Option<&'a str> }
#[derive(Serialize)] struct OsvPkg<'a> { name: &'a str, ecosystem: &'a str }
async fn osv(client: &reqwest::Client, cache: &mut Cache, eco: Ecosystem, name: &str, version: Option<&str>) -> Option<Vec<String>> {
    let key = format!("{}:{}@{}", eco.osv_name(), name, version.unwrap_or("*"));
    if let Some((ids, ts)) = cache.osv.get(&key) { if now().saturating_sub(*ts) < TTL_OSV { return Some(ids.clone()); } }
    let resp = client.post("https://api.osv.dev/v1/query").json(&OsvQuery { package: OsvPkg { name, ecosystem: eco.osv_name() }, version }).send().await.ok()?;
    if !resp.status().is_success() { return None; }
    let v: serde_json::Value = resp.json().await.ok()?;
    let ids: Vec<String> = v["vulns"].as_array().map(|a| a.iter().filter_map(|x| { let id = x["id"].as_str()?; let summary = x["summary"].as_str().unwrap_or(""); let sev = x["database_specific"]["severity"].as_str().unwrap_or(""); Some(if summary.is_empty() { id.to_string() } else { format!("{id}{}: {summary}", if sev.is_empty() { String::new() } else { format!(" [{sev}]") }) }) }).collect()).unwrap_or_default();
    cache.osv.insert(key, (ids.clone(), now()));
    Some(ids)
}

/// Damerau-Levenshtein distance capped for short names.
pub fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect(); let b: Vec<char> = b.chars().collect();
    let (n, m) = (a.len(), b.len());
    let mut d = vec![vec![0usize; m + 1]; n + 1];
    for i in 0..=n { d[i][0] = i; } for j in 0..=m { d[0][j] = j; }
    for i in 1..=n { for j in 1..=m { let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 }; d[i][j] = (d[i - 1][j] + 1).min(d[i][j - 1] + 1).min(d[i - 1][j - 1] + cost); if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] { d[i][j] = d[i][j].min(d[i - 2][j - 2] + 1); } } }
    d[n][m]
}
pub fn near_miss(eco: Ecosystem, name: &str) -> Option<&'static str> {
    let pool: &[&str] = match eco { Ecosystem::Npm => POPULAR_NPM, Ecosystem::PyPi => POPULAR_PYPI, Ecosystem::Crates => POPULAR_CRATES, Ecosystem::Go => return None };
    if pool.contains(&name) { return None; }
    let n = name.to_lowercase();
    pool.iter().copied().filter(|p| p.len() >= 4 && n.len() >= 4).find(|p| { let d = edit_distance(&n, p); d >= 1 && d <= if p.len() > 7 { 2 } else { 1 } })
}

pub async fn run(ctx: &CheckCtx, check: &mut VerifyCheck) -> Result<()> {
    let refs = extract(&ctx.scope.files);
    if refs.is_empty() { check.status = "pass".into(); check.summary = "no new dependencies introduced".into(); check.metric("deps", 0.0); return Ok(()); }
    let decl = declared(&ctx.root);
    // Imports of already-declared packages are grounded by the manifest; only manifests + undeclared imports hit registries.
    let mut to_resolve: Vec<DepRef> = vec![];
    let mut grounded = 0usize;
    for mut r in refs {
        if !r.from_manifest {
            let declared_here = match r.eco { Ecosystem::Npm => decl.npm.contains(&r.name), Ecosystem::Crates => decl.crates.contains(&r.name) || decl.crates.contains(&r.name.replace('-', "_")), Ecosystem::PyPi => { if let Some(dist) = decl.import_map.get(&r.name) { r.name = dist.clone(); } decl.pypi.contains(&r.name) || decl.pypi.contains(&normalize_pypi(&r.name)) || local_py_module(&ctx.root, &r.name) }, Ecosystem::Go => decl.go.contains(&r.name) || decl.go.iter().any(|g| r.name.starts_with(g)) };
            if declared_here { grounded += 1; continue; }
            // Local-module heuristics: a Rust `use foo::` for a sibling module in this crate, or an npm workspace package.
            if r.eco == Ecosystem::Crates && (ctx.root.join("src").join(format!("{}.rs", r.name.replace('-', "_"))).exists() || ctx.root.join("src").join(r.name.replace('-', "_")).is_dir() || ctx.root.join("src-tauri/src").join(format!("{}.rs", r.name.replace('-', "_"))).exists()) { grounded += 1; continue; }
            if r.eco == Ecosystem::Npm && ctx.root.join("node_modules").join(&r.name).exists() { grounded += 1; continue; }
        }
        to_resolve.push(r);
    }
    check.metric("deps", (to_resolve.len() + grounded) as f64);
    let mut lock_missing = 0usize;
    for r in &to_resolve {
        let lock = match r.eco { Ecosystem::Npm => decl.lock_npm.as_ref(), Ecosystem::Crates => decl.lock_crates.as_ref(), Ecosystem::Go => decl.lock_go.as_ref(), Ecosystem::PyPi => None };
        if let Some(lock) = lock { if !lock.contains(&r.name) && !lock.contains(&r.name.replace('-', "_")) && !lock.iter().any(|l| r.eco == Ecosystem::Go && l.starts_with(&r.name)) { lock_missing += 1; check.findings.push(VerifyFinding::new("medium", format!("`{}` is not in the lockfile — run the package manager install so the lock matches", r.name)).at(&r.path, r.line).rule("dep-not-in-lockfile").suggest(match r.eco { Ecosystem::Npm => "npm install".to_string(), Ecosystem::Crates => "cargo update -p ".to_string() + &r.name, Ecosystem::Go => "go mod tidy".into(), Ecosystem::PyPi => String::new() })); } }
        if let Some(p) = near_miss(r.eco, &r.name) { check.findings.push(VerifyFinding::new("high", format!("`{}` looks like a near-miss of the popular package `{}` (possible hallucinated/typosquatted name)", r.name, p)).at(&r.path, r.line).rule("dep-near-miss").suggest(format!("did you mean `{p}`?"))); }
    }
    if ctx.privacy() { check.summary = format!("{} dependenc{} checked offline (privacy mode: registry/OSV lookups skipped){}", to_resolve.len() + grounded, if to_resolve.len() + grounded == 1 { "y" } else { "ies" }, if lock_missing > 0 { format!(", {lock_missing} not in lockfile") } else { String::new() }); check.settle(false); return Ok(()); }
    let client = reqwest::Client::builder().timeout(Duration::from_secs(8)).connect_timeout(Duration::from_secs(4)).user_agent("caret-verify/0.1").build()?;
    let mut cache = load_cache();
    let mut missing = 0usize; let mut vulns = 0usize; let mut unknown = 0usize; let mut checked = 0usize;
    for r in to_resolve.iter().take(60) {
        if ctx.cancel.is_cancelled() { break; }
        checked += 1;
        match exists(&client, &mut cache, r.eco, &r.name).await {
            Some(true) => {}
            Some(false) => { missing += 1; check.findings.push(VerifyFinding::new("critical", format!("`{}` does not exist on {}", r.name, r.eco.label())).at(&r.path, r.line).rule("dep-not-found").suggest("the agent may have hallucinated this package; remove it or pick a real one")); continue; }
            None => { unknown += 1; check.findings.push(VerifyFinding::new("info", format!("could not reach {} to verify `{}`", r.eco.label(), r.name)).at(&r.path, r.line).rule("dep-unverified")); continue; }
        }
        if let Some(ids) = osv(&client, &mut cache, r.eco, &r.name, r.version.as_deref()).await { if !ids.is_empty() { vulns += 1; let sev = if r.version.is_some() { "high" } else { "medium" }; check.findings.push(VerifyFinding::new(sev, format!("`{}`{} has {} known advisor{}: {}", r.name, r.version.as_ref().map(|v| format!("@{v}")).unwrap_or_default(), ids.len(), if ids.len() == 1 { "y" } else { "ies" }, ids.iter().take(3).cloned().collect::<Vec<_>>().join("; "))).at(&r.path, r.line).rule("dep-vulnerable").suggest("upgrade to a patched version (see osv.dev)")); } }
    }
    save_cache(&cache);
    check.metric("resolved", checked as f64); check.metric("missing", missing as f64); check.metric("vulnerable", vulns as f64);
    let total = checked + grounded;
    check.summary = format!("{total} dependenc{} grounded{}{}{}{}", if total == 1 { "y" } else { "ies" }, if missing > 0 { format!(", {missing} not found") } else { String::new() }, if vulns > 0 { format!(", {vulns} with advisories") } else { String::new() }, if lock_missing > 0 { format!(", {lock_missing} not in lockfile") } else { String::new() }, if unknown > 0 { format!(", {unknown} unreachable") } else { String::new() });
    check.settle(false);
    Ok(())
}
fn local_py_module(root: &std::path::Path, name: &str) -> bool { root.join(format!("{name}.py")).exists() || root.join(name).join("__init__.py").exists() || root.join("src").join(format!("{name}.py")).exists() || root.join("src").join(name).join("__init__.py").exists() }

#[cfg(test)]
mod t {
    use super::*;
    use crate::ai::agent::session::ChangeKind;
    fn tf(path: &str, after: &str, added: Vec<(u32, &str)>) -> TouchedFile { TouchedFile { path: path.into(), abs: std::path::PathBuf::from(path), before: None, after: Some(after.into()), added_lines: added.into_iter().map(|(l, s)| (l, s.to_string())).collect(), removed_lines: vec![], diff: String::new(), lang: crate::verify::scope::lang_of(path), class: crate::verify::scope::classify(path), kind: ChangeKind::Modify } }
    #[test]
    fn npm_names() {
        assert_eq!(npm_package_name("@scope/pkg/sub").as_deref(), Some("@scope/pkg"));
        assert_eq!(npm_package_name("lodash/fp").as_deref(), Some("lodash"));
        assert_eq!(npm_package_name("./local"), None); assert_eq!(npm_package_name("node:fs"), None); assert_eq!(npm_package_name("fs"), None);
        assert_eq!(npm_package_name("monaco-editor/esm/vs/editor/editor.worker?worker").as_deref(), Some("monaco-editor"));
    }
    #[test]
    fn extracts_imports_and_manifests() {
        let files = vec![
            tf("src/a.ts", "import x from 'leftpad-pro';\nimport { y } from './b';\nconst z = require('chalk');", vec![(1, "import x from 'leftpad-pro';"), (2, "import { y } from './b';"), (3, "const z = require('chalk');")]),
            tf("app.py", "import requests, os\nfrom bs4 import BeautifulSoup", vec![(1, "import requests, os"), (2, "from bs4 import BeautifulSoup")]),
            tf("package.json", r#"{"dependencies":{"express":"^4.0.0"},"devDependencies":{"vitest":"1.0.0"}}"#, vec![]),
            tf("Cargo.toml", "[package]\nname=\"x\"\n[dependencies]\nserde = { version = \"1\", features = [\"derive\"] }\ntokio = \"1\"\nlocal = { path = \"../local\" }\n", vec![]),
        ];
        let refs = extract(&files);
        let names: Vec<(Ecosystem, &str)> = refs.iter().map(|r| (r.eco, r.name.as_str())).collect();
        assert!(names.contains(&(Ecosystem::Npm, "leftpad-pro"))); assert!(names.contains(&(Ecosystem::Npm, "chalk"))); assert!(!names.iter().any(|(_, n)| *n == "./b"));
        assert!(names.contains(&(Ecosystem::PyPi, "requests"))); assert!(names.contains(&(Ecosystem::PyPi, "bs4"))); assert!(!names.iter().any(|(_, n)| *n == "os"));
        assert!(names.contains(&(Ecosystem::Npm, "express"))); assert!(names.contains(&(Ecosystem::Npm, "vitest")));
        assert!(names.contains(&(Ecosystem::Crates, "serde"))); assert!(names.contains(&(Ecosystem::Crates, "tokio"))); assert!(!names.iter().any(|(_, n)| *n == "local"));
        let express = refs.iter().find(|r| r.name == "express").unwrap(); assert_eq!(express.version.as_deref(), Some("4.0.0"));
    }
    #[test]
    fn near_misses() {
        assert_eq!(near_miss(Ecosystem::Npm, "lodahs"), Some("lodash"));
        assert_eq!(near_miss(Ecosystem::PyPi, "requets"), Some("requests"));
        assert_eq!(near_miss(Ecosystem::Npm, "lodash"), None);
        assert_eq!(near_miss(Ecosystem::Npm, "zustand"), None);
        assert_eq!(near_miss(Ecosystem::Crates, "serde_json"), None);
        assert_eq!(edit_distance("kitten", "sitting"), 3); assert_eq!(edit_distance("ab", "ba"), 1);
    }
}
