use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use serde::Deserialize;
use serde_json::{json, Map, Value};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use or_io::container::rpf::crypto::RpfCryptoProvider;
use or_title::{build_mounted_game, GtaInstall, MountedGame};

use super::census_cap::{Cap, CapUnit};
use super::keys::resolve_crypto;
use super::search::{
    apply_filter, pack_infos, query_mounted, resolve_cutoff, resolve_game_root, search_raw,
    split_exts, HitFilter, Matcher, DEFAULT_MAX_HITS,
};
use super::wardrobe_exchange;

pub(super) const MAX_INLINE_BYTES: usize = 8 * 1024 * 1024;

const MAX_REQUEST_BYTES: usize = 16 * 1024 * 1024;

#[derive(Default)]
pub(super) struct Session {
    default_game: Option<PathBuf>,
    default_keys: Option<PathBuf>,
    default_dlc: Option<String>,
    game_root: Option<PathBuf>,
    crypto: Option<Arc<dyn RpfCryptoProvider>>,
    dlc_label: Option<String>,
    mounted: Option<MountedGame>,
}

#[derive(Deserialize)]
struct Request {
    #[serde(default)]
    id: Option<Value>,
    cmd: String,
    #[serde(flatten)]
    params: Map<String, Value>,
}

pub fn run(args: &[String]) -> ExitCode {
    let (default_game, default_keys, default_dlc) = match parse_flags(args) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Error: {e}");
            eprintln!();
            print_usage();
            return ExitCode::from(2);
        }
    };

    init_stderr_tracing();

    let mut session = Session {
        default_game,
        default_keys,
        default_dlc,
        game_root: None,
        crypto: None,
        dlc_label: None,
        mounted: None,
    };

    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    let mut reader = stdin.lock();
    let mut raw = Vec::new();

    loop {
        raw.clear();
        match read_capped_line(&mut reader, &mut raw) {
            LineRead::Eof => break,
            LineRead::Empty => continue,
            LineRead::TooLong => {
                let resp = error_response(
                    None,
                    format!("request exceeds {MAX_REQUEST_BYTES} byte limit; discarded"),
                );
                if !write_line(&mut out, &resp) {
                    break;
                }
                continue;
            }
            LineRead::Line => {}
        }
        let line = String::from_utf8_lossy(&raw);
        let (response, shutdown) = handle_line(&line, &mut session);
        if !write_line(&mut out, &response) {
            break;
        }
        if shutdown {
            break;
        }
    }

    ExitCode::SUCCESS
}

pub(super) fn write_line(out: &mut impl Write, response: &Value) -> bool {
    let encoded = serde_json::to_string(response)
        .unwrap_or_else(|e| format!("{{\"ok\":false,\"error\":\"serialize response: {e}\"}}"));
    writeln!(out, "{encoded}").is_ok() && out.flush().is_ok()
}

pub(super) fn handle_line(line: &str, session: &mut Session) -> (Value, bool) {
    let id = serde_json::from_str::<Value>(line)
        .ok()
        .and_then(|v| v.get("id").cloned());

    let req: Request = match serde_json::from_str(line) {
        Ok(r) => r,
        Err(e) => {
            return (error_response(id, format!("invalid request: {e}")), false);
        }
    };
    let id = req.id.clone();

    let dispatched =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| dispatch(req, session)));
    match dispatched {
        Ok((result, shutdown)) => {
            let response = match result {
                Ok(body) => ok_response(id, body),
                Err(e) => error_response(id, e),
            };
            (response, shutdown)
        }
        Err(panic) => {
            let msg = panic_message(&panic);
            (
                error_response(id, format!("internal error (handler panicked): {msg}")),
                false,
            )
        }
    }
}

fn dispatch(req: Request, session: &mut Session) -> (Result<Value, String>, bool) {
    let mut shutdown = false;
    let result = match req.cmd.as_str() {
        "ping" => Ok(json!({ "pong": true, "version": env!("CARGO_PKG_VERSION") })),
        "mount" => do_mount(&req.params, session),
        "status" => Ok(do_status(session)),
        "unmount" => {
            session.mounted = None;
            session.game_root = None;
            session.crypto = None;
            session.dlc_label = None;
            Ok(json!({}))
        }
        "search" => do_search(&req.params, session),
        "packs" => do_packs(session),
        "list" => do_list(&req.params, session),
        "locate" => do_locate(&req.params, session),
        "extract" => do_extract(&req.params, session),
        "extract-many" | "extract_many" => do_extract_many(&req.params, session),
        "wardrobe-add" => wardrobe_exchange::add(&req.params),
        "wardrobe-set" => wardrobe_exchange::set(&req.params),
        "wardrobe-fix" => wardrobe_exchange::fix(&req.params),
        "wardrobe-waive" => wardrobe_exchange::waive(&req.params),
        "shutdown" => {
            shutdown = true;
            Ok(json!({}))
        }
        #[cfg(test)]
        "__panic" => panic!("test panic"),
        other => Err(format!("unknown command: {other}")),
    };
    (result, shutdown)
}

fn panic_message(panic: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = panic.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = panic.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown".to_string()
    }
}

pub(super) enum LineRead {
    Line,
    Empty,
    Eof,
    TooLong,
}

pub(super) fn read_capped_line(reader: &mut impl BufRead, buf: &mut Vec<u8>) -> LineRead {
    let mut too_long = false;
    let mut saw_any = false;
    loop {
        let chunk = match reader.fill_buf() {
            Ok(c) => c,
            Err(_) => return LineRead::Eof,
        };
        if chunk.is_empty() {
            if !saw_any {
                return LineRead::Eof;
            }
            break;
        }
        saw_any = true;
        let (data, consumed, done) = match chunk.iter().position(|&b| b == b'\n') {
            Some(nl) => (&chunk[..nl], nl + 1, true),
            None => (chunk, chunk.len(), false),
        };
        if !too_long {
            if buf.len() + data.len() > MAX_REQUEST_BYTES {
                too_long = true;
            } else {
                buf.extend_from_slice(data);
            }
        }
        reader.consume(consumed);
        if done {
            break;
        }
    }
    if too_long {
        buf.clear();
        return LineRead::TooLong;
    }
    while buf.last() == Some(&b'\r') {
        buf.pop();
    }
    if buf.iter().all(|b| b.is_ascii_whitespace()) {
        LineRead::Empty
    } else {
        LineRead::Line
    }
}

fn do_mount(params: &Map<String, Value>, session: &mut Session) -> Result<Value, String> {
    let game = str_param(params, "game")
        .map(PathBuf::from)
        .or_else(|| session.default_game.clone());
    let keys = str_param(params, "keys")
        .map(PathBuf::from)
        .or_else(|| session.default_keys.clone());
    let dlc = str_param(params, "dlc").or_else(|| session.default_dlc.clone());

    let game_root = resolve_game_root(game)?;
    if !game_root.is_dir() {
        return Err(format!(
            "game root is not a directory: {}",
            game_root.display()
        ));
    }

    let resolved = resolve_crypto(&game_root, keys.as_deref());
    let provider = resolved.provider;
    let mut warnings = resolved.notes;

    let install = GtaInstall::from_path(&game_root)
        .ok_or_else(|| format!("invalid install root: {}", game_root.display()))?;
    let cutoff = resolve_cutoff(dlc.as_deref());
    let game = build_mounted_game(&install, Arc::clone(&provider), cutoff)
        .map_err(|e| format!("mount failed: {e}"))?;
    warnings.extend(game.diagnostics.messages().iter().map(|d| d.to_string()));

    let packs = game.packs.len();
    let roots = game.roots.len();
    let edition = game.game.asset_slug();
    let dlc_label = dlc.unwrap_or_else(|| "all".to_string());

    session.game_root = Some(game_root.clone());
    session.crypto = Some(provider);
    session.dlc_label = Some(dlc_label.clone());
    session.mounted = Some(game);

    Ok(json!({
        "gameRoot": game_root.display().to_string(),
        "edition": edition,
        "dlc": dlc_label,
        "packs": packs,
        "roots": roots,
        "warnings": warnings,
    }))
}

fn do_status(session: &Session) -> Value {
    json!({
        "mounted": session.mounted.is_some(),
        "gameRoot": session.game_root.as_ref().map(|p| p.display().to_string()),
        "edition": session.mounted.as_ref().map(|g| g.game.asset_slug()),
        "dlc": session.dlc_label,
        "packs": session.mounted.as_ref().map(|g| g.packs.len()),
        "roots": session.mounted.as_ref().map(|g| g.roots.len()),
    })
}

fn do_search(params: &Map<String, Value>, session: &mut Session) -> Result<Value, String> {
    let pattern = str_param(params, "pattern");
    let file_types = str_array_upper(params, "fileTypes");
    let raw = bool_param(params, "raw").unwrap_or(false);
    let limit = usize_param(params, "limit");
    let offset = usize_param(params, "offset").unwrap_or(0);
    let count_only = bool_param(params, "countOnly").unwrap_or(false);
    let filter = HitFilter {
        ext: ext_param(params),
        device: str_param(params, "device").map(|d| d.to_ascii_lowercase()),
    };

    // ⚠ One client asking for everything decides how much memory the process
    // holds for the rest of the session. `maxHits` raises the ceiling per
    // request; the response says whether the sweep stopped short.
    let cap = match usize_param(params, "maxHits") {
        Some(0) => {
            return Err(
                "maxHits: 0 is not a ceiling. Omit it for the default, or pass a \
                        count — a whole install can hold more hits than this machine"
                    .into(),
            )
        }
        Some(n) => Cap::at(n as u64, CapUnit::Count("hit(s)")),
        None => Cap::new(DEFAULT_MAX_HITS, CapUnit::Count("hit(s)")),
    };

    if raw && filter.device.is_some() {
        return Err("device is only valid in mounted search (raw paths have no device)".into());
    }
    if pattern.is_none() && file_types.is_empty() && filter.ext.is_empty() {
        return Err("provide a name pattern, ext, or fileTypes (mounted mode)".into());
    }
    let matcher = Matcher::new(pattern);

    let (mut hits, warnings) = if raw {
        if !file_types.is_empty() {
            return Err("fileTypes is only valid in mounted search (omit raw)".into());
        }
        let game_root = match &session.game_root {
            Some(r) => r.clone(),
            None => resolve_game_root(session.default_game.clone())?,
        };
        let provider = match &session.crypto {
            Some(c) => Arc::clone(c),
            None => resolve_crypto(&game_root, session.default_keys.as_deref()).provider,
        };
        let mut w = Vec::new();
        let h = search_raw(&game_root, provider, &matcher, cap, &mut w)?;
        (h, w)
    } else {
        let game = session
            .mounted
            .as_ref()
            .ok_or("not mounted; call `mount` first (or use raw:true)")?;
        query_mounted(game, &file_types, &matcher, cap)
    };

    // ⚠ Before the filter: filtering can drop a truncated list below its own
    // ceiling, and the client would then be told the answer is complete.
    let (truncated, max_hits) = cap.json(hits.len() as u64);

    apply_filter(&mut hits, &filter);
    let count = hits.len();
    if count_only {
        return Ok(json!({
            "count": count,
            "truncated": truncated,
            "maxHits": max_hits,
            "warnings": warnings,
        }));
    }
    paginate(&mut hits, offset, limit);

    // ⚠ `truncated` is the sweep's own ceiling and is not `offset`/`limit`.
    // A paginated page is a window onto a complete answer; a truncated sweep
    // is an incomplete answer, and a client that conflates the two reports a
    // total that is not one.
    Ok(json!({
        "count": count,
        "hits": hits,
        "truncated": truncated,
        "maxHits": max_hits,
        "warnings": warnings,
    }))
}

fn paginate<T>(items: &mut Vec<T>, offset: usize, limit: Option<usize>) {
    if offset > 0 {
        if offset >= items.len() {
            items.clear();
        } else {
            items.drain(0..offset);
        }
    }
    if let Some(l) = limit {
        items.truncate(l);
    }
}

fn do_packs(session: &Session) -> Result<Value, String> {
    let game = session
        .mounted
        .as_ref()
        .ok_or("not mounted; call `mount` first")?;
    let packs = pack_infos(game);
    Ok(json!({ "count": packs.len(), "packs": packs }))
}

fn do_list(params: &Map<String, Value>, session: &Session) -> Result<Value, String> {
    let path = str_param(params, "path").ok_or("list: 'path' is required")?;
    let game = session
        .mounted
        .as_ref()
        .ok_or("not mounted; call `mount` first")?;
    Ok(json!({ "entries": game.vfs.list(&path) }))
}

fn do_locate(params: &Map<String, Value>, session: &Session) -> Result<Value, String> {
    let path = str_param(params, "path").ok_or("locate: 'path' is required")?;
    let game = session
        .mounted
        .as_ref()
        .ok_or("not mounted; call `mount` first")?;
    match game.vfs.locate(&path) {
        Some(container) => Ok(json!({ "container": container })),
        None => Err(format!("not resolvable: {path}")),
    }
}

fn do_extract(params: &Map<String, Value>, session: &Session) -> Result<Value, String> {
    let path = str_param(params, "path").ok_or("extract: 'path' is required")?;
    let game = session
        .mounted
        .as_ref()
        .ok_or("not mounted; call `mount` first")?;
    let bytes = game
        .vfs
        .open(&path)
        .ok_or_else(|| format!("not found: {path}"))?;

    if let Some(out) = str_param(params, "out") {
        std::fs::write(&out, &bytes).map_err(|e| format!("write {out}: {e}"))?;
        Ok(json!({ "bytes": bytes.len(), "out": out }))
    } else if bool_param(params, "inline").unwrap_or(false) {
        if bytes.len() > MAX_INLINE_BYTES {
            return Err(format!(
                "file too large for inline ({} bytes > {MAX_INLINE_BYTES}); use 'out'",
                bytes.len()
            ));
        }
        Ok(json!({ "bytes": bytes.len(), "data": base64_encode(&bytes) }))
    } else {
        Err("extract: provide 'out' (dest file) or inline:true".into())
    }
}

fn do_extract_many(params: &Map<String, Value>, session: &Session) -> Result<Value, String> {
    let paths = params
        .get("paths")
        .and_then(Value::as_array)
        .ok_or("extract-many: 'paths' (array of strings) is required")?;
    let out_dir = str_param(params, "outDir").ok_or("extract-many: 'outDir' is required")?;
    let game = session
        .mounted
        .as_ref()
        .ok_or("not mounted; call `mount` first")?;

    let out_root = PathBuf::from(&out_dir);
    std::fs::create_dir_all(&out_root).map_err(|e| format!("create {out_dir}: {e}"))?;

    let mut written = Vec::new();
    let mut errors = Vec::new();
    for p in paths {
        let Some(path) = p.as_str() else {
            errors.push(json!({ "path": p, "error": "not a string" }));
            continue;
        };
        match extract_one_to_dir(game, path, &out_root) {
            Ok((out, bytes)) => written.push(json!({
                "path": path,
                "out": out.display().to_string(),
                "bytes": bytes,
            })),
            Err(e) => errors.push(json!({ "path": path, "error": e })),
        }
    }
    Ok(json!({
        "requested": paths.len(),
        "written": written,
        "errors": errors,
    }))
}

fn extract_one_to_dir(
    game: &MountedGame,
    path: &str,
    out_root: &std::path::Path,
) -> Result<(PathBuf, usize), String> {
    let bytes = game
        .vfs
        .open(path)
        .ok_or_else(|| format!("not found: {path}"))?;

    let dest = extract_dest_path(out_root, path)?;
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
    }
    std::fs::write(&dest, &bytes).map_err(|e| format!("write {}: {e}", dest.display()))?;
    Ok((dest, bytes.len()))
}

fn extract_dest_path(out_root: &std::path::Path, vfs_path: &str) -> Result<PathBuf, String> {
    let (device, rel) = vfs_path
        .split_once(":/")
        .ok_or_else(|| format!("not a device path: {vfs_path}"))?;
    let device_dir = device_mount_folder(device);
    let rel = rel.replace('\\', "/");
    let mut dest = out_root.to_path_buf();
    dest.push(device_dir);
    let mut saw_part = false;
    for part in rel.split('/') {
        if part.is_empty() || part == "." || part == ".." || part.contains(':') {
            continue;
        }
        dest.push(part);
        saw_part = true;
    }
    if !saw_part {
        return Err(format!("no usable file name in: {vfs_path}"));
    }
    Ok(dest)
}

fn device_mount_folder(device: &str) -> String {
    let mut name = device.trim().trim_end_matches(':').to_string();
    if name.len() > 3 && name.to_ascii_lowercase().ends_with("crc") {
        name.truncate(name.len() - 3);
    }
    name.replace([':', '\\', '/'], "_")
}

fn ok_response(id: Option<Value>, body: Value) -> Value {
    let mut m = Map::new();
    if let Some(id) = id {
        m.insert("id".into(), id);
    }
    m.insert("ok".into(), Value::Bool(true));
    if let Value::Object(o) = body {
        for (k, v) in o {
            m.insert(k, v);
        }
    }
    Value::Object(m)
}

fn error_response(id: Option<Value>, message: String) -> Value {
    let mut m = Map::new();
    if let Some(id) = id {
        m.insert("id".into(), id);
    }
    m.insert("ok".into(), Value::Bool(false));
    m.insert("error".into(), Value::String(message));
    Value::Object(m)
}

fn str_param(params: &Map<String, Value>, key: &str) -> Option<String> {
    params
        .get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn bool_param(params: &Map<String, Value>, key: &str) -> Option<bool> {
    params.get(key).and_then(|v| v.as_bool())
}

fn usize_param(params: &Map<String, Value>, key: &str) -> Option<usize> {
    params.get(key).and_then(|v| v.as_u64()).map(|n| n as usize)
}

fn ext_param(params: &Map<String, Value>) -> Vec<String> {
    match params.get("ext") {
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(|x| x.as_str())
            .flat_map(split_exts)
            .collect(),
        Some(Value::String(s)) => split_exts(s),
        _ => Vec::new(),
    }
}

fn str_array_upper(params: &Map<String, Value>, key: &str) -> Vec<String> {
    params
        .get(key)
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str())
                .map(|s| s.trim().to_ascii_uppercase())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

pub(super) fn base64_encode(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[((n >> 18) & 63) as usize] as char);
        out.push(TABLE[((n >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            TABLE[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

type Flags = (Option<PathBuf>, Option<PathBuf>, Option<String>);

fn parse_flags(args: &[String]) -> Result<Flags, String> {
    let mut game = None;
    let mut keys = None;
    let mut dlc = None;
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        let mut take = |flag: &str| -> Result<String, String> {
            i += 1;
            args.get(i)
                .cloned()
                .ok_or_else(|| format!("{flag} needs a value"))
        };
        match arg.as_str() {
            "-h" | "--help" | "help" => {
                print_usage();
                std::process::exit(0);
            }
            "--game" => game = Some(PathBuf::from(take("--game")?)),
            "--keys" => keys = Some(PathBuf::from(take("--keys")?)),
            "--dlc" => dlc = Some(take("--dlc")?),
            other => {
                if let Some(v) = other.strip_prefix("--game=") {
                    game = Some(PathBuf::from(v));
                } else if let Some(v) = other.strip_prefix("--keys=") {
                    keys = Some(PathBuf::from(v));
                } else if let Some(v) = other.strip_prefix("--dlc=") {
                    dlc = Some(v.to_string());
                } else {
                    return Err(format!("unknown option: {other}"));
                }
            }
        }
        i += 1;
    }
    Ok((game, keys, dlc))
}

pub(super) fn init_stderr_tracing() {
    let _ = tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .with(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("or_io=warn".parse().unwrap())
                .add_directive("openrage_cli=info".parse().unwrap()),
        )
        .try_init();
}

fn print_usage() {
    eprintln!("Persistent NDJSON server: mount a GTA install once, serve many commands.");
    eprintln!();
    eprintln!("Usage: openrage-cli serve [--game <dir>] [--keys <file>] [--dlc <X>]");
    eprintln!();
    eprintln!("Reads one JSON request per line on stdin, writes one JSON response per");
    eprintln!("line on stdout; logs go to stderr. Flags set defaults for `mount`.");
    eprintln!();
    eprintln!("Commands (cmd field): ping, mount, status, unmount, search, packs, list,");
    eprintln!("locate, extract, extract-many, shutdown, and the four that change a pack:");
    eprintln!("wardrobe-add, wardrobe-set, wardrobe-fix, wardrobe-waive.");
    eprintln!("See docs/reference/CLI_Server_Protocol.md for the reference.");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_session() -> Session {
        Session {
            default_game: None,
            default_keys: None,
            default_dlc: None,
            game_root: None,
            crypto: None,
            dlc_label: None,
            mounted: None,
        }
    }

    #[test]
    fn ping_returns_pong_and_version() {
        let mut s = new_session();
        let (resp, shutdown) = handle_line(r#"{"id":1,"cmd":"ping"}"#, &mut s);
        assert!(!shutdown);
        assert_eq!(resp["id"], json!(1));
        assert_eq!(resp["ok"], json!(true));
        assert_eq!(resp["pong"], json!(true));
        assert!(resp["version"].is_string());
    }

    #[test]
    fn search_before_mount_errors() {
        let mut s = new_session();
        let (resp, _) = handle_line(r#"{"id":2,"cmd":"search","pattern":"x"}"#, &mut s);
        assert_eq!(resp["ok"], json!(false));
        assert!(resp["error"].as_str().unwrap().contains("not mounted"));
    }

    #[test]
    fn search_requires_pattern_or_filetypes() {
        let mut s = new_session();
        let (resp, _) = handle_line(r#"{"cmd":"search"}"#, &mut s);
        assert_eq!(resp["ok"], json!(false));
        assert!(resp["error"].as_str().unwrap().contains("pattern"));
    }

    #[test]
    fn unknown_command_errors() {
        let mut s = new_session();
        let (resp, _) = handle_line(r#"{"id":"a","cmd":"frobnicate"}"#, &mut s);
        assert_eq!(resp["id"], json!("a"));
        assert_eq!(resp["ok"], json!(false));
        assert!(resp["error"].as_str().unwrap().contains("unknown command"));
    }

    #[test]
    fn invalid_json_errors_without_id() {
        let mut s = new_session();
        let (resp, _) = handle_line("{not json", &mut s);
        assert_eq!(resp["ok"], json!(false));
        assert!(resp.get("id").is_none());
    }

    #[test]
    fn shutdown_signals_exit() {
        let mut s = new_session();
        let (resp, shutdown) = handle_line(r#"{"cmd":"shutdown"}"#, &mut s);
        assert!(shutdown);
        assert_eq!(resp["ok"], json!(true));
    }

    #[test]
    fn status_reports_unmounted() {
        let mut s = new_session();
        let (resp, _) = handle_line(r#"{"cmd":"status"}"#, &mut s);
        assert_eq!(resp["ok"], json!(true));
        assert_eq!(resp["mounted"], json!(false));
    }

    #[test]
    fn packs_before_mount_errors() {
        let mut s = new_session();
        let (resp, _) = handle_line(r#"{"cmd":"packs"}"#, &mut s);
        assert_eq!(resp["ok"], json!(false));
        assert!(resp["error"].as_str().unwrap().contains("not mounted"));
    }

    #[test]
    fn search_accepts_ext_only() {
        let mut s = new_session();
        let (resp, _) = handle_line(r#"{"cmd":"search","ext":["ypt"]}"#, &mut s);
        assert_eq!(resp["ok"], json!(false));
        assert!(resp["error"].as_str().unwrap().contains("not mounted"));
    }

    #[test]
    fn search_rejects_device_with_raw() {
        let mut s = new_session();
        let (resp, _) = handle_line(
            r#"{"cmd":"search","pattern":"x","raw":true,"device":"common"}"#,
            &mut s,
        );
        assert_eq!(resp["ok"], json!(false));
        assert!(resp["error"].as_str().unwrap().contains("mounted search"));
    }

    #[test]
    fn ext_param_accepts_array_and_string() {
        let arr: Map<String, Value> =
            serde_json::from_str(r#"{"ext":[".Meta","YPT, ytd"]}"#).unwrap();
        assert_eq!(ext_param(&arr), vec!["meta", "ypt", "ytd"]);
        let s: Map<String, Value> = serde_json::from_str(r#"{"ext":"meta,.ypt"}"#).unwrap();
        assert_eq!(ext_param(&s), vec!["meta", "ypt"]);
        let none: Map<String, Value> = serde_json::from_str(r#"{}"#).unwrap();
        assert!(ext_param(&none).is_empty());
    }

    #[test]
    fn paginate_offset_and_limit() {
        let base: Vec<i32> = (0..10).collect();

        let mut v = base.clone();
        paginate(&mut v, 0, None);
        assert_eq!(v.len(), 10);

        let mut v = base.clone();
        paginate(&mut v, 3, Some(4));
        assert_eq!(v, vec![3, 4, 5, 6]);

        let mut v = base.clone();
        paginate(&mut v, 20, None);
        assert!(v.is_empty());

        let mut v = base.clone();
        paginate(&mut v, 8, Some(5));
        assert_eq!(v, vec![8, 9]);
    }

    #[test]
    fn device_mount_folder_strips_crc_suffix() {
        assert_eq!(device_mount_folder("commoncrc"), "common");
        assert_eq!(device_mount_folder("commonCRC:"), "common");
        assert_eq!(device_mount_folder("dlc_mpHeist3CRC"), "dlc_mpHeist3");
        assert_eq!(device_mount_folder("update"), "update");
    }

    #[test]
    fn extract_dest_path_prefixes_by_device() {
        let root = PathBuf::from("out");
        let p = extract_dest_path(&root, "commoncrc:/data/ai/weapons.meta").unwrap();
        assert_eq!(p, PathBuf::from("out/common/data/ai/weapons.meta"));
        let p = extract_dest_path(
            &root,
            "dlc_mpHeist3CRC:/common/data/ai/weapon_navyrevolver.meta",
        )
        .unwrap();
        assert_eq!(
            p,
            PathBuf::from("out/dlc_mpHeist3/common/data/ai/weapon_navyrevolver.meta")
        );
    }

    #[test]
    fn extract_many_before_mount_errors() {
        let mut s = new_session();
        let (resp, _) = handle_line(
            r#"{"cmd":"extract-many","paths":["a:/b.meta"],"outDir":"x"}"#,
            &mut s,
        );
        assert_eq!(resp["ok"], json!(false));
        assert!(resp["error"].as_str().unwrap().contains("not mounted"));
    }

    #[test]
    fn extract_many_requires_paths_and_out_dir() {
        let mut s = new_session();
        let (resp, _) = handle_line(r#"{"cmd":"extract-many"}"#, &mut s);
        assert_eq!(resp["ok"], json!(false));
        assert!(resp["error"].as_str().unwrap().contains("paths"));

        let (resp, _) = handle_line(r#"{"cmd":"extract-many","paths":[]}"#, &mut s);
        assert_eq!(resp["ok"], json!(false));
        assert!(resp["error"].as_str().unwrap().contains("outDir"));
    }

    #[test]
    fn read_capped_line_splits_and_caps() {
        use std::io::Cursor;
        let mut input = Cursor::new(b"{\"a\":1}\r\n\n  \nsecond\n".to_vec());
        let mut buf = Vec::new();

        assert!(matches!(
            read_capped_line(&mut input, &mut buf),
            LineRead::Line
        ));
        assert_eq!(buf, b"{\"a\":1}");

        buf.clear();
        assert!(matches!(
            read_capped_line(&mut input, &mut buf),
            LineRead::Empty
        ));
        buf.clear();
        assert!(matches!(
            read_capped_line(&mut input, &mut buf),
            LineRead::Empty
        ));

        buf.clear();
        assert!(matches!(
            read_capped_line(&mut input, &mut buf),
            LineRead::Line
        ));
        assert_eq!(buf, b"second");

        buf.clear();
        assert!(matches!(
            read_capped_line(&mut input, &mut buf),
            LineRead::Eof
        ));

        let mut long = vec![b'x'; MAX_REQUEST_BYTES + 1];
        long.push(b'\n');
        long.extend_from_slice(b"after\n");
        let mut input = Cursor::new(long);
        buf.clear();
        assert!(matches!(
            read_capped_line(&mut input, &mut buf),
            LineRead::TooLong
        ));
        buf.clear();
        assert!(matches!(
            read_capped_line(&mut input, &mut buf),
            LineRead::Line
        ));
        assert_eq!(buf, b"after");
    }

    #[test]
    #[cfg_attr(
        all(windows, debug_assertions),
        ignore = "Cranelift cannot unwind a panic on Windows: run it with `cargo test --release`"
    )]
    fn handler_panic_returns_error_envelope() {
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let mut s = new_session();
        let (resp, shutdown) = handle_line(r#"{"id":7,"cmd":"__panic"}"#, &mut s);
        std::panic::set_hook(prev);

        assert!(!shutdown);
        assert_eq!(resp["id"], json!(7));
        assert_eq!(resp["ok"], json!(false));
        let err = resp["error"].as_str().unwrap();
        assert!(err.contains("internal error"), "got: {err}");
        assert!(err.contains("test panic"), "got: {err}");
    }

    /// A project whose one clothing content is an empty pack, and a folder of two jackets named as a pack names them.
    fn staged(dir: &std::path::Path) -> (String, String) {
        let pack = dir.join("pack");
        or_wardrobe::edit::create(&pack, "demo").unwrap();
        let container = or_project::Container {
            modules: vec![or_project::Module::clothing("wardrobe", "pack")],
            outputs: vec![or_project::Output::fivem_resource(
                "fivem",
                "demo",
                &["wardrobe"],
            )],
        };
        let project = dir.join("demo.openrage-proj");
        or_project::Project::create(&project, &container, &[]).unwrap();
        let drop = dir.join("drop");
        std::fs::create_dir_all(&drop).unwrap();
        let c = "mp_m_freemode_01_mp_m_demo";
        for n in 0..2 {
            let model = or_source::sample::drawables_of(&format!("jacket{n}"));
            let texture = or_source::sample::textures_of(&format!("jacket{n}"));
            std::fs::write(drop.join(format!("{c}^jbib_00{n}_u.ydd")), model).unwrap();
            std::fs::write(drop.join(format!("{c}^jbib_diff_00{n}_a_uni.ytd")), texture).unwrap();
        }
        (project.display().to_string(), drop.display().to_string())
    }

    fn ask(session: &mut Session, request: Value) -> Value {
        handle_line(&request.to_string(), session).0
    }

    #[test]
    fn a_script_adds_sets_fixes_and_waives_and_the_pack_changes_as_each_request_states() {
        let dir = tempfile::tempdir().unwrap();
        let (project, drop) = staged(dir.path());
        let pack = dir.path().join("pack");
        let mut s = new_session();

        // Add: the two jackets of the folder are garments, and a model is a source.
        let added = ask(
            &mut s,
            json!({"id":1,"cmd":"wardrobe-add","project":project,"from":[drop]}),
        );
        assert_eq!(added["ok"], json!(true), "{added}");
        assert_eq!(added["id"], json!(1));
        let ids: Vec<String> = serde_json::from_value(added["garments"].clone()).unwrap();
        assert_eq!(ids.len(), 2, "{added}");
        let wardrobe = or_wardrobe::Wardrobe::read(&pack).unwrap();
        assert_eq!(wardrobe.garments.len(), 2);
        assert!(wardrobe.garments[0]
            .model
            .as_deref()
            .unwrap()
            .ends_with(".drawables.src"));

        // Set: both jackets are pinned at place 0, the second named and tagged, and a request that sets nothing is refused.
        let first = Some(0);
        let pinned = ask(
            &mut s,
            json!({"cmd":"wardrobe-set","project":project,"garments":[ids[0]],"position":0}),
        );
        assert_eq!(pinned["ok"], json!(true), "{pinned}");
        let set = ask(
            &mut s,
            json!({"cmd":"wardrobe-set","project":project,"garments":[ids[1]],"position":0,"label":"second","addTag":"wool"}),
        );
        assert_eq!(set["ok"], json!(true), "{set}");
        let wardrobe = or_wardrobe::Wardrobe::read(&pack).unwrap();
        let second = wardrobe.garment(&ids[1]).unwrap();
        assert_eq!(
            (second.position, second.label.as_deref()),
            (first, Some("second"))
        );
        assert_eq!(second.tags, ["wool"]);
        let nothing = ask(
            &mut s,
            json!({"cmd":"wardrobe-set","project":project,"garments":[ids[1]]}),
        );
        assert_eq!(nothing["ok"], json!(false));

        // Fix: two garments pinned at one place is a clash, and the later one is let go of its pin.
        let fixed = ask(&mut s, json!({"cmd":"wardrobe-fix","project":project}));
        assert_eq!(fixed["ok"], json!(true), "{fixed}");
        assert_eq!(fixed["changed"], json!(true), "{fixed}");
        let wardrobe = or_wardrobe::Wardrobe::read(&pack).unwrap();
        assert_eq!(wardrobe.garment(&ids[0]).unwrap().position, first);
        assert_eq!(wardrobe.garment(&ids[1]).unwrap().position, None);
        let again = ask(&mut s, json!({"cmd":"wardrobe-fix","project":project}));
        assert_eq!(
            again["changed"],
            json!(false),
            "a second run changes nothing: {again}"
        );

        // Waive: a reason is recorded, and none is refused.
        let code = or_wardrobe::RULES.iter().find(|r| r.waivable).unwrap().code;
        let waived = ask(
            &mut s,
            json!({"cmd":"wardrobe-waive","project":project,"code":code,"reason":"the shop wants it"}),
        );
        assert_eq!(waived["ok"], json!(true), "{waived}");
        assert_eq!(or_wardrobe::Wardrobe::read(&pack).unwrap().waivers.len(), 1);
        let reasonless = ask(
            &mut s,
            json!({"cmd":"wardrobe-waive","project":project,"code":code,"reason":" "}),
        );
        assert_eq!(reasonless["ok"], json!(false));
        assert_eq!(or_wardrobe::Wardrobe::read(&pack).unwrap().waivers.len(), 1);
    }

    #[test]
    fn a_route_that_changes_a_pack_names_a_project_and_says_so_when_it_is_missing() {
        let mut s = new_session();
        let none = ask(&mut s, json!({"cmd":"wardrobe-fix"}));
        assert_eq!(none["ok"], json!(false));
        assert!(
            none["error"].as_str().unwrap().contains("project"),
            "{none}"
        );
        let gone = ask(
            &mut s,
            json!({"cmd":"wardrobe-add","project":"nowhere.openrage-proj","from":["x"]}),
        );
        assert_eq!(gone["ok"], json!(false));
    }

    #[test]
    fn base64_matches_known_vectors() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }
}
