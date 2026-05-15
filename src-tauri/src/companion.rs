#![allow(unexpected_cfgs)]

use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use tokio::net::TcpListener;
use tokio::sync::broadcast;
use tokio_tungstenite::{accept_async, tungstenite::Message};

const COMPANION_MANIFEST_FALLBACK: &str =
    include_str!("../resources/share/companion/skills/first-party.json");
const FOCUS_CHANGED_EVENT: &str = "focus-changed";
const COMPANION_STATE_CHANGED_EVENT: &str = "companion-state-changed";
const COMPANION_TOGGLE_REQUESTED_EVENT: &str = "companion-toggle-requested";
const COMPANION_FIGMA_BRIDGE_EVENT: &str = "companion-figma-bridge";
const COMPANION_GRANTS_FILE: &str = "companion-skill-grants.json";
const FIGMA_BRIDGE_PORT: u16 = 19796;
const MAIN_WINDOW_LABEL: &str = "main";
const COMPANION_WINDOW_LABEL: &str = "companion";
const COMPANION_MCP_HOST_COMMAND: &str = "entropic-companion-mcp";
const COMPANION_MCP_CLI_FLAG: &str = "--entropic-companion-mcp";
const ENTROPIC_BUNDLE_IDS: &[&str] = &["ai.openclaw.entropic", "ai.openclaw.entropic.dev"];

static COMPANION_FOCUS: OnceLock<Mutex<CompanionFocusCache>> = OnceLock::new();
static COMPANION_APP_HANDLE: OnceLock<AppHandle> = OnceLock::new();
static FIGMA_BRIDGE_STATE: OnceLock<Mutex<FigmaBridgeState>> = OnceLock::new();
static FIGMA_BRIDGE_STARTED: OnceLock<()> = OnceLock::new();
static FIGMA_COMMAND_TX: OnceLock<broadcast::Sender<String>> = OnceLock::new();

#[derive(Debug, Clone, Default)]
struct CompanionFocusCache {
    current: Option<CompanionFocusSnapshot>,
    host: Option<CompanionFocusSnapshot>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct FigmaBridgeState {
    connected: bool,
    client_count: usize,
    updated_at_ms: Option<u128>,
    last_payload: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompanionFocusSnapshot {
    pub supported: bool,
    pub bundle_id: Option<String>,
    pub app_name: Option<String>,
    pub window_title: Option<String>,
    pub pid: Option<i32>,
    pub focused_at_ms: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompanionPermissionManifest {
    pub id: String,
    pub label: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompanionToolManifest {
    pub id: String,
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub input_schema: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompanionTransportManifest {
    pub kind: String,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub port: Option<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompanionSkillManifest {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub bundle_ids: Vec<String>,
    pub activation: String,
    pub transport: CompanionTransportManifest,
    pub context_mode: String,
    #[serde(default)]
    pub unit_of_work: Vec<String>,
    #[serde(default)]
    pub permissions: Vec<CompanionPermissionManifest>,
    #[serde(default)]
    pub tools: Vec<CompanionToolManifest>,
    pub system_prompt_fragment: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CompanionManifestFile {
    #[allow(dead_code)]
    version: u32,
    skills: Vec<CompanionSkillManifest>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompanionRuntimeSkill {
    pub id: String,
    pub name: String,
    pub bundle_ids: Vec<String>,
    pub activation: String,
    pub transport: CompanionTransportManifest,
    pub context_mode: String,
    pub unit_of_work: Vec<String>,
    pub permissions: Vec<CompanionPermissionManifest>,
    pub tools: Vec<CompanionToolManifest>,
    pub system_prompt_fragment: String,
    pub granted: bool,
    pub status: String,
    pub status_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompanionState {
    pub focus: Option<CompanionFocusSnapshot>,
    pub host_focus: Option<CompanionFocusSnapshot>,
    pub primary_skill: Option<CompanionRuntimeSkill>,
    pub background_skills: Vec<CompanionRuntimeSkill>,
    pub skills: Vec<CompanionRuntimeSkill>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompanionToolRunRequest {
    pub skill_id: String,
    pub tool_id: String,
    #[serde(default)]
    pub input: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompanionToolRunResult {
    pub skill_id: String,
    pub tool_id: String,
    pub summary: String,
    pub output: serde_json::Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct CompanionGrantsFile {
    #[serde(default)]
    grants: HashMap<String, bool>,
}

pub fn init(app: &AppHandle) -> Result<(), String> {
    let _ = COMPANION_APP_HANDLE.set(app.clone());
    install_companion_tray(app)?;
    install_companion_hotkey(app)?;
    install_focus_listener(app)?;
    start_figma_bridge(app);

    if let Some(snapshot) = frontmost_focus_snapshot() {
        update_focus_cache(app, snapshot);
    }

    Ok(())
}

pub fn maybe_handle_cli_mode() -> Option<i32> {
    let mut args = std::env::args().skip(1);
    let flag = args.next()?;
    if flag != COMPANION_MCP_CLI_FLAG {
        return None;
    }
    Some(run_companion_mcp_stdio_host(args.collect()))
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

fn focus_cache() -> &'static Mutex<CompanionFocusCache> {
    COMPANION_FOCUS.get_or_init(|| Mutex::new(CompanionFocusCache::default()))
}

fn figma_bridge_state() -> &'static Mutex<FigmaBridgeState> {
    FIGMA_BRIDGE_STATE.get_or_init(|| Mutex::new(FigmaBridgeState::default()))
}

fn figma_bridge_snapshot() -> FigmaBridgeState {
    figma_bridge_state()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

fn figma_command_sender() -> &'static broadcast::Sender<String> {
    FIGMA_COMMAND_TX.get_or_init(|| {
        let (tx, _rx) = broadcast::channel(64);
        tx
    })
}

fn is_entropic_bundle(bundle_id: Option<&str>) -> bool {
    bundle_id
        .map(|id| {
            ENTROPIC_BUNDLE_IDS
                .iter()
                .any(|candidate| candidate.eq_ignore_ascii_case(id))
        })
        .unwrap_or(false)
}

fn update_focus_cache(app: &AppHandle, snapshot: CompanionFocusSnapshot) {
    {
        let mut cache = focus_cache()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if !is_entropic_bundle(snapshot.bundle_id.as_deref()) {
            cache.host = Some(snapshot.clone());
        }
        cache.current = Some(snapshot.clone());
    }

    let _ = app.emit(FOCUS_CHANGED_EVENT, &snapshot);
    if let Ok(state) = build_companion_state(app) {
        let _ = app.emit(COMPANION_STATE_CHANGED_EVENT, state);
    }
}

fn companion_resource_skill_dir(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .resource_dir()
        .ok()
        .map(|base| base.join("share").join("companion").join("skills"))
}

fn parse_manifest_file(raw: &str) -> Result<Vec<CompanionSkillManifest>, String> {
    serde_json::from_str::<CompanionManifestFile>(raw)
        .map(|file| file.skills)
        .map_err(|error| format!("Invalid Companion skill manifest: {error}"))
}

fn fallback_skill_manifests() -> Vec<CompanionSkillManifest> {
    parse_manifest_file(COMPANION_MANIFEST_FALLBACK).unwrap_or_default()
}

fn load_skill_manifests(app: &AppHandle) -> Vec<CompanionSkillManifest> {
    let Some(dir) = companion_resource_skill_dir(app) else {
        return fallback_skill_manifests();
    };

    let mut skills = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            match fs::read_to_string(&path)
                .ok()
                .and_then(|raw| parse_manifest_file(&raw).ok())
            {
                Some(mut loaded) => skills.append(&mut loaded),
                None => eprintln!(
                    "[Entropic] Failed to load Companion manifest {}",
                    path.display()
                ),
            }
        }
    }

    if skills.is_empty() {
        fallback_skill_manifests()
    } else {
        skills
    }
}

fn companion_grants_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("Failed to resolve app data dir: {error}"))?;
    fs::create_dir_all(&dir).map_err(|error| format!("Failed to create app data dir: {error}"))?;
    Ok(dir.join(COMPANION_GRANTS_FILE))
}

fn load_companion_grants(app: &AppHandle) -> CompanionGrantsFile {
    let Ok(path) = companion_grants_path(app) else {
        return CompanionGrantsFile::default();
    };
    let Ok(raw) = fs::read_to_string(path) else {
        return CompanionGrantsFile::default();
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

fn save_companion_grants(app: &AppHandle, grants: &CompanionGrantsFile) -> Result<(), String> {
    let path = companion_grants_path(app)?;
    let raw = serde_json::to_string_pretty(grants)
        .map_err(|error| format!("Failed to serialize Companion grants: {error}"))?;
    fs::write(path, raw).map_err(|error| format!("Failed to write Companion grants: {error}"))
}

fn skill_requires_grant(skill: &CompanionSkillManifest) -> bool {
    !skill.permissions.is_empty()
}

fn runtime_skill(
    skill: CompanionSkillManifest,
    grants: &CompanionGrantsFile,
) -> CompanionRuntimeSkill {
    let requires_grant = skill_requires_grant(&skill);
    let granted = !requires_grant || grants.grants.get(&skill.id).copied().unwrap_or(false);
    let (status, status_reason) = if granted {
        match skill.transport.kind.as_str() {
            "figma_plugin_ws" => {
                let figma = figma_bridge_snapshot();
                if figma.connected {
                    ("ready".to_string(), None)
                } else {
                    (
                        "waiting_for_bridge".to_string(),
                        Some("Install and run the Entropic Figma plugin to connect the local bridge.".to_string()),
                    )
                }
            }
            "mcp_stdio" => {
                match skill
                    .transport
                    .command
                    .as_deref()
                    .filter(|command| companion_transport_command_available(command))
                {
                    Some(_) => ("ready".to_string(), None),
                    None => (
                        "missing_dependency".to_string(),
                        Some("The Companion MCP server command is not available.".to_string()),
                    ),
                }
            }
            _ => ("ready".to_string(), None),
        }
    } else {
        (
            "needs_grant".to_string(),
            Some("Enable this skill before it can read or write app content.".to_string()),
        )
    };

    CompanionRuntimeSkill {
        id: skill.id,
        name: skill.name,
        bundle_ids: skill.bundle_ids,
        activation: skill.activation,
        transport: skill.transport,
        context_mode: skill.context_mode,
        unit_of_work: skill.unit_of_work,
        permissions: skill.permissions,
        tools: skill.tools,
        system_prompt_fragment: skill.system_prompt_fragment,
        granted,
        status,
        status_reason,
    }
}

fn companion_transport_command_available(command: &str) -> bool {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed == COMPANION_MCP_HOST_COMMAND {
        return true;
    }
    if trimmed.contains('/') {
        return PathBuf::from(trimmed).exists();
    }
    which::which(trimmed).is_ok()
}

fn skill_matches_focus(
    skill: &CompanionRuntimeSkill,
    focus: Option<&CompanionFocusSnapshot>,
) -> bool {
    let Some(bundle_id) = focus.and_then(|snapshot| snapshot.bundle_id.as_deref()) else {
        return false;
    };
    skill
        .bundle_ids
        .iter()
        .any(|candidate| candidate.eq_ignore_ascii_case(bundle_id))
}

fn build_companion_state(app: &AppHandle) -> Result<CompanionState, String> {
    if let Some(snapshot) = frontmost_focus_snapshot() {
        update_focus_cache_without_emit(snapshot);
    }

    let cache = focus_cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
    let grants = load_companion_grants(app);
    let skills: Vec<CompanionRuntimeSkill> = load_skill_manifests(app)
        .into_iter()
        .map(|skill| runtime_skill(skill, &grants))
        .collect();
    let host_focus = cache.host.clone().or_else(|| {
        cache
            .current
            .clone()
            .filter(|focus| !is_entropic_bundle(focus.bundle_id.as_deref()))
    });
    let focus_for_skill = host_focus.as_ref().or(cache.current.as_ref());
    let primary_skill = skills
        .iter()
        .find(|skill| skill.activation == "focus" && skill_matches_focus(skill, focus_for_skill))
        .cloned();
    let background_skills = skills
        .iter()
        .filter(|skill| skill.activation == "background")
        .cloned()
        .collect();

    Ok(CompanionState {
        focus: cache.current,
        host_focus,
        primary_skill,
        background_skills,
        skills,
    })
}

fn update_focus_cache_without_emit(snapshot: CompanionFocusSnapshot) {
    let mut cache = focus_cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if !is_entropic_bundle(snapshot.bundle_id.as_deref()) {
        cache.host = Some(snapshot.clone());
    }
    cache.current = Some(snapshot);
}

#[tauri::command]
pub async fn get_companion_state(app: AppHandle) -> Result<CompanionState, String> {
    build_companion_state(&app)
}

#[tauri::command]
pub async fn set_companion_skill_grant(
    app: AppHandle,
    skill_id: String,
    granted: bool,
) -> Result<CompanionState, String> {
    let skill_id = skill_id.trim().to_string();
    if skill_id.is_empty() {
        return Err("A Companion skill id is required.".to_string());
    }

    let manifests = load_skill_manifests(&app);
    if !manifests.iter().any(|skill| skill.id == skill_id) {
        return Err(format!("Unknown Companion skill: {skill_id}"));
    }

    let mut grants = load_companion_grants(&app);
    grants.grants.insert(skill_id, granted);
    save_companion_grants(&app, &grants)?;
    let state = build_companion_state(&app)?;
    let _ = app.emit(COMPANION_STATE_CHANGED_EVENT, &state);
    Ok(state)
}

#[tauri::command]
pub async fn companion_run_tool(
    app: AppHandle,
    request: CompanionToolRunRequest,
) -> Result<CompanionToolRunResult, String> {
    let skill_id = request.skill_id.trim().to_string();
    let tool_id = request.tool_id.trim().to_string();
    if skill_id.is_empty() || tool_id.is_empty() {
        return Err("Companion skill id and tool id are required.".to_string());
    }

    let manifests = load_skill_manifests(&app);
    let manifest = manifests
        .iter()
        .find(|skill| skill.id == skill_id)
        .ok_or_else(|| format!("Unknown Companion skill: {skill_id}"))?;
    if !manifest.tools.iter().any(|tool| tool.id == tool_id) {
        return Err(format!("Skill {skill_id} does not expose tool {tool_id}."));
    }

    let grants = load_companion_grants(&app);
    if skill_requires_grant(manifest) && !grants.grants.get(&skill_id).copied().unwrap_or(false) {
        return Err(format!(
            "Enable {name} before using its tools.",
            name = manifest.name
        ));
    }

    match tool_id.as_str() {
        "get_focus_context" => {
            let state = build_companion_state(&app)?;
            Ok(CompanionToolRunResult {
                skill_id,
                tool_id,
                summary: "Focus context captured.".to_string(),
                output: serde_json::to_value(state)
                    .map_err(|error| format!("Failed to encode Companion state: {error}"))?,
            })
        }
        "get_selection" => {
            run_selection_probe(manifest, &request.input).map(|output| CompanionToolRunResult {
                skill_id,
                tool_id,
                summary: "Selection probe completed.".to_string(),
                output,
            })
        }
        "create_frame" if manifest.transport.kind == "figma_plugin_ws" => {
            figma_create_frame(&request.input).map(|output| CompanionToolRunResult {
                skill_id,
                tool_id,
                summary: "Figma frame creation command queued.".to_string(),
                output,
            })
        }
        _ if manifest.transport.kind == "mcp_stdio" => {
            run_mcp_stdio_tool(manifest, &tool_id, &request.input).map(|output| {
                CompanionToolRunResult {
                    skill_id,
                    tool_id,
                    summary: "Companion MCP tool completed.".to_string(),
                    output,
                }
            })
        }
        _ => Err(format!(
            "Tool {tool_id} is declared for {skill_id}, but no local executor is available yet."
        )),
    }
}

#[tauri::command]
pub async fn show_main_window(app: AppHandle) -> Result<(), String> {
    show_main_window_inner(&app)
}

#[tauri::command]
pub async fn show_companion_window(app: AppHandle) -> Result<(), String> {
    show_companion_window_inner(&app)
}

#[tauri::command]
pub async fn hide_companion_window(app: AppHandle) -> Result<(), String> {
    hide_companion_window_inner(&app)
}

#[tauri::command]
pub async fn toggle_companion_window(app: AppHandle) -> Result<(), String> {
    toggle_companion_window_inner(&app)
}

fn run_selection_probe(
    manifest: &CompanionSkillManifest,
    _input: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    match manifest.transport.kind.as_str() {
        "builtin_jxa" => {
            let script = selection_probe_script(&manifest.id, &manifest.name)?;
            run_jxa_json(&script)
        }
        "figma_plugin_ws" => figma_selection_probe(),
        "mcp_stdio" => run_mcp_stdio_tool(manifest, "get_selection", _input),
        other => Err(format!(
            "Unsupported Companion transport for selection probe: {other}"
        )),
    }
}

fn figma_selection_probe() -> Result<serde_json::Value, String> {
    let state = figma_bridge_snapshot();
    if !state.connected {
        return Err("The Figma Companion plugin is not connected to the local bridge.".to_string());
    }
    Ok(json!({
        "available": state.last_payload.is_some(),
        "app": "Figma",
        "grain": "frame_component_variant_auto_layout_node",
        "connected": state.connected,
        "clientCount": state.client_count,
        "updatedAtMs": state.updated_at_ms,
        "payload": state.last_payload,
    }))
}

fn figma_create_frame(input: &serde_json::Value) -> Result<serde_json::Value, String> {
    let state = figma_bridge_snapshot();
    if !state.connected {
        return Err("The Figma Companion plugin is not connected to the local bridge.".to_string());
    }

    let command_id = format!("create-frame-{}", now_ms());
    let command = json!({
        "type": "create_frame",
        "id": command_id,
        "name": input
            .get("name")
            .and_then(|value| value.as_str())
            .unwrap_or("Entropic Frame"),
        "x": input.get("x").and_then(|value| value.as_f64()).unwrap_or(0.0),
        "y": input.get("y").and_then(|value| value.as_f64()).unwrap_or(0.0),
        "width": input
            .get("width")
            .and_then(|value| value.as_f64())
            .unwrap_or(720.0),
        "height": input
            .get("height")
            .and_then(|value| value.as_f64())
            .unwrap_or(480.0),
        "updatedAt": now_ms(),
    });

    figma_command_sender()
        .send(command.to_string())
        .map_err(|_| "No Figma Companion plugin client is ready for commands.".to_string())?;

    Ok(json!({
        "queued": true,
        "command": command,
    }))
}

fn run_mcp_stdio_tool(
    manifest: &CompanionSkillManifest,
    tool_id: &str,
    input: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let command = manifest
        .transport
        .command
        .as_deref()
        .map(str::trim)
        .filter(|command| !command.is_empty())
        .ok_or_else(|| {
            format!(
                "Companion skill {} does not declare an MCP command.",
                manifest.id
            )
        })?;
    if !companion_transport_command_available(command) {
        return Err(format!(
            "Companion MCP command for {skill} is not available: {command}",
            skill = manifest.name
        ));
    }

    let mut process = if command == COMPANION_MCP_HOST_COMMAND {
        let exe = std::env::current_exe()
            .map_err(|error| format!("Failed to resolve Entropic executable: {error}"))?;
        let mut process = Command::new(exe);
        process.arg(COMPANION_MCP_CLI_FLAG);
        process
    } else {
        Command::new(command)
    };
    process.args(&manifest.transport.args);
    if manifest.transport.args.is_empty() {
        process.arg(&manifest.id);
    }

    let mut child = process
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("Failed to start Companion MCP server {command}: {error}"))?;

    let stdin = child
        .stdin
        .as_mut()
        .ok_or_else(|| "Companion MCP server stdin is unavailable.".to_string())?;
    let initialize = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {
                "name": "entropic-companion",
                "version": env!("CARGO_PKG_VERSION")
            }
        }
    });
    let initialized = json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized",
        "params": {}
    });
    let call = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": tool_id,
            "arguments": input,
        }
    });
    writeln!(stdin, "{initialize}")
        .and_then(|_| writeln!(stdin, "{initialized}"))
        .and_then(|_| writeln!(stdin, "{call}"))
        .and_then(|_| stdin.flush())
        .map_err(|error| format!("Failed to send Companion MCP request: {error}"))?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "Companion MCP server stdout is unavailable.".to_string())?;
    let reader = BufReader::new(stdout);
    let mut initialize_seen = false;

    for line in reader.lines().take(256) {
        let line =
            line.map_err(|error| format!("Failed to read Companion MCP response: {error}"))?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let message: serde_json::Value = match serde_json::from_str(trimmed) {
            Ok(value) => value,
            Err(_) => continue,
        };
        if message.get("id").and_then(|value| value.as_i64()) == Some(1) {
            initialize_seen = true;
            if let Some(error) = message.get("error") {
                let _ = child.kill();
                return Err(format!("Companion MCP initialize failed: {error}"));
            }
            continue;
        }
        if message.get("id").and_then(|value| value.as_i64()) != Some(2) {
            continue;
        }

        let _ = child.kill();
        if let Some(error) = message.get("error") {
            return Err(format!("Companion MCP tool failed: {error}"));
        }
        return Ok(json!({
            "transport": "mcp_stdio",
            "initialized": initialize_seen,
            "result": message.get("result").cloned().unwrap_or(serde_json::Value::Null),
        }));
    }

    let mut stderr_text = String::new();
    if let Some(stderr) = child.stderr.take() {
        let mut stderr_reader = BufReader::new(stderr);
        let _ = stderr_reader.read_line(&mut stderr_text);
    }
    let _ = child.kill();
    Err(if stderr_text.trim().is_empty() {
        "Companion MCP server ended before returning a tool result.".to_string()
    } else {
        format!(
            "Companion MCP server ended before returning a tool result: {}",
            stderr_text.trim()
        )
    })
}

fn run_companion_mcp_stdio_host(args: Vec<String>) -> i32 {
    let skill_id = match parse_companion_mcp_skill_id(&args) {
        Some(skill_id) => skill_id,
        None => {
            eprintln!("[Entropic] Companion MCP host requires a skill id.");
            return 2;
        }
    };
    let manifest = match fallback_skill_manifests()
        .into_iter()
        .find(|skill| skill.id == skill_id)
    {
        Some(manifest) => manifest,
        None => {
            eprintln!("[Entropic] Unknown Companion MCP skill: {skill_id}");
            return 2;
        }
    };

    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(line) => line,
            Err(error) => {
                eprintln!("[Entropic] Companion MCP stdin read failed: {error}");
                return 1;
            }
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let request: serde_json::Value = match serde_json::from_str(trimmed) {
            Ok(request) => request,
            Err(error) => {
                let response = mcp_error(serde_json::Value::Null, -32700, &error.to_string());
                let _ = writeln!(stdout, "{response}");
                let _ = stdout.flush();
                continue;
            }
        };
        let Some(response) = handle_companion_mcp_request(&manifest, &request) else {
            continue;
        };
        if writeln!(stdout, "{response}")
            .and_then(|_| stdout.flush())
            .is_err()
        {
            return 1;
        }
    }

    0
}

fn parse_companion_mcp_skill_id(args: &[String]) -> Option<String> {
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--skill" {
            return iter.next().map(|value| value.trim().to_string());
        }
        if !arg.starts_with('-') {
            return Some(arg.trim().to_string());
        }
    }
    None
}

fn handle_companion_mcp_request(
    manifest: &CompanionSkillManifest,
    request: &serde_json::Value,
) -> Option<serde_json::Value> {
    let method = request.get("method").and_then(|value| value.as_str())?;
    if method.starts_with("notifications/") {
        return None;
    }

    let id = request
        .get("id")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    match method {
        "initialize" => Some(mcp_result(
            id,
            json!({
                "protocolVersion": "2024-11-05",
                "serverInfo": {
                    "name": format!("entropic-companion-{}", manifest.id),
                    "version": env!("CARGO_PKG_VERSION")
                },
                "capabilities": {
                    "tools": {}
                }
            }),
        )),
        "tools/list" => Some(mcp_result(
            id,
            json!({
                "tools": manifest.tools.iter().map(|tool| {
                    json!({
                        "name": tool.id,
                        "description": tool.description,
                        "inputSchema": tool.input_schema.clone().unwrap_or_else(|| json!({
                            "type": "object",
                            "properties": {}
                        }))
                    })
                }).collect::<Vec<_>>()
            }),
        )),
        "tools/call" => {
            let Some(params) = request.get("params") else {
                return Some(mcp_error(id, -32602, "Missing tools/call params."));
            };
            let Some(tool_id) = params.get("name").and_then(|value| value.as_str()) else {
                return Some(mcp_error(id, -32602, "Missing tools/call name."));
            };
            let input = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            match run_first_party_mcp_tool(manifest, tool_id, &input) {
                Ok(output) => {
                    let text = serde_json::to_string_pretty(&output)
                        .unwrap_or_else(|_| output.to_string());
                    Some(mcp_result(
                        id,
                        json!({
                            "content": [
                                {
                                    "type": "text",
                                    "text": text
                                }
                            ],
                            "structuredContent": output
                        }),
                    ))
                }
                Err(error) => Some(mcp_error(id, -32000, &error)),
            }
        }
        _ => Some(mcp_error(
            id,
            -32601,
            &format!("Unknown MCP method: {method}"),
        )),
    }
}

fn run_first_party_mcp_tool(
    manifest: &CompanionSkillManifest,
    tool_id: &str,
    _input: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    if !manifest.tools.iter().any(|tool| tool.id == tool_id) {
        return Err(format!(
            "Skill {skill} does not expose tool {tool_id}.",
            skill = manifest.id
        ));
    }

    match tool_id {
        "get_focus_context" => Ok(json!({
            "available": true,
            "skill": {
                "id": manifest.id,
                "name": manifest.name,
                "bundleIds": manifest.bundle_ids,
                "activation": manifest.activation,
                "contextMode": manifest.context_mode,
                "unitOfWork": manifest.unit_of_work,
                "tools": manifest.tools,
            }
        })),
        "get_selection" => {
            let script = selection_probe_script(&manifest.id, &manifest.name)?;
            run_jxa_json(&script)
        }
        _ => Err(format!(
            "Tool {tool_id} is declared for {skill}, but the first-party MCP host does not implement it yet.",
            skill = manifest.id
        )),
    }
}

fn mcp_result(id: serde_json::Value, result: serde_json::Value) -> serde_json::Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result
    })
}

fn mcp_error(id: serde_json::Value, code: i64, message: &str) -> serde_json::Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": code,
            "message": message
        }
    })
}

fn start_figma_bridge(app: &AppHandle) {
    if FIGMA_BRIDGE_STARTED.set(()).is_err() {
        return;
    }

    let app_handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let listener = match TcpListener::bind(("127.0.0.1", FIGMA_BRIDGE_PORT)).await {
            Ok(listener) => listener,
            Err(error) => {
                eprintln!(
                    "[Entropic] Failed to start Companion Figma bridge on 127.0.0.1:{FIGMA_BRIDGE_PORT}: {error}"
                );
                return;
            }
        };
        eprintln!("[Entropic] Companion Figma bridge listening on 127.0.0.1:{FIGMA_BRIDGE_PORT}");

        loop {
            let Ok((stream, _addr)) = listener.accept().await else {
                continue;
            };
            let app_for_client = app_handle.clone();
            tauri::async_runtime::spawn(async move {
                let Ok(ws) = accept_async(stream).await else {
                    return;
                };
                let (mut write, mut read) = ws.split();
                let mut command_rx = figma_command_sender().subscribe();

                {
                    let mut state = figma_bridge_state()
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    state.connected = true;
                    state.client_count = state.client_count.saturating_add(1);
                    state.updated_at_ms = Some(now_ms());
                    let _ = app_for_client.emit(COMPANION_FIGMA_BRIDGE_EVENT, state.clone());
                }

                let hello = json!({
                    "type": "hello",
                    "source": "entropic-companion",
                    "port": FIGMA_BRIDGE_PORT,
                });
                let _ = write.send(Message::Text(hello.to_string())).await;

                loop {
                    tokio::select! {
                        command = command_rx.recv() => {
                            let Ok(command) = command else {
                                continue;
                            };
                            if write.send(Message::Text(command)).await.is_err() {
                                break;
                            }
                        }
                        message = read.next() => {
                            let Some(Ok(message)) = message else {
                                break;
                            };
                            let text = match message {
                                Message::Text(text) => text,
                                Message::Binary(bytes) => String::from_utf8(bytes).unwrap_or_default(),
                                Message::Close(_) => break,
                                _ => continue,
                            };
                            let payload: serde_json::Value =
                                serde_json::from_str(&text).unwrap_or_else(|_| json!({ "raw": text }));
                            {
                                let mut state = figma_bridge_state()
                                    .lock()
                                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                                state.connected = true;
                                state.updated_at_ms = Some(now_ms());
                                state.last_payload = Some(payload);
                                let _ = app_for_client.emit(COMPANION_FIGMA_BRIDGE_EVENT, state.clone());
                            }
                            if let Ok(state) = build_companion_state(&app_for_client) {
                                let _ = app_for_client.emit(COMPANION_STATE_CHANGED_EVENT, state);
                            }
                        }
                    }
                }

                {
                    let mut state = figma_bridge_state()
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    state.client_count = state.client_count.saturating_sub(1);
                    state.connected = state.client_count > 0;
                    state.updated_at_ms = Some(now_ms());
                    let _ = app_for_client.emit(COMPANION_FIGMA_BRIDGE_EVENT, state.clone());
                }
                if let Ok(state) = build_companion_state(&app_for_client) {
                    let _ = app_for_client.emit(COMPANION_STATE_CHANGED_EVENT, state);
                }
            });
        }
    });
}

fn run_jxa_json(script: &str) -> Result<serde_json::Value, String> {
    if which::which("osascript").is_err() {
        return Err("osascript is not available on this machine.".to_string());
    }
    let output = Command::new("osascript")
        .arg("-l")
        .arg("JavaScript")
        .arg("-e")
        .arg(script)
        .output()
        .map_err(|error| format!("Failed to run osascript: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            "osascript failed without stderr.".to_string()
        } else {
            stderr
        });
    }
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if stdout.is_empty() {
        return Ok(json!({ "available": false, "reason": "empty_probe_output" }));
    }
    serde_json::from_str(&stdout).or_else(|_| Ok(json!({ "raw": stdout })))
}

fn selection_probe_script(skill_id: &str, app_name: &str) -> Result<String, String> {
    let script = match skill_id {
        "powerpoint" => {
            r#"
(() => {
  const app = Application("Microsoft PowerPoint");
  if (!app.running()) return JSON.stringify({ available: false, app: "PowerPoint", reason: "not_running" });
  const result = { available: true, app: "PowerPoint", grain: "slide" };
  try {
    const window = app.activeWindow();
    const selection = window.selection();
    try { result.selectionType = String(selection.selectionType()); } catch (_) {}
    try {
      const slides = selection.slideRange();
      result.slideCount = slides.length;
      result.slides = slides().map((slide) => ({ index: slide.slideIndex(), name: String(slide.name()) }));
    } catch (error) {
      result.slideError = String(error);
    }
    try {
      const shapes = selection.shapeRange();
      result.shapeCount = shapes.length;
      result.shapes = shapes().map((shape) => ({ name: String(shape.name()) }));
    } catch (_) {}
  } catch (error) {
    result.available = false;
    result.error = String(error);
  }
  return JSON.stringify(result);
})()
"#
        }
        "excel" => {
            r#"
(() => {
  const app = Application("Microsoft Excel");
  if (!app.running()) return JSON.stringify({ available: false, app: "Excel", reason: "not_running" });
  const result = { available: true, app: "Excel", grain: "range" };
  try {
    const selection = app.selection();
    try { result.address = String(selection.address()); } catch (_) {}
    try { result.value = selection.value(); } catch (_) {}
    try { result.formula = selection.formula(); } catch (_) {}
    try { result.worksheet = String(app.activeSheet().name()); } catch (_) {}
    try { result.workbook = String(app.activeWorkbook().name()); } catch (_) {}
  } catch (error) {
    result.available = false;
    result.error = String(error);
  }
  return JSON.stringify(result);
})()
"#
        }
        "word" => {
            r#"
(() => {
  const app = Application("Microsoft Word");
  if (!app.running()) return JSON.stringify({ available: false, app: "Word", reason: "not_running" });
  const result = { available: true, app: "Word", grain: "paragraph" };
  try {
    const selection = app.selection();
    try { result.text = String(selection.content()); } catch (_) {}
    try { result.document = String(app.activeDocument().name()); } catch (_) {}
  } catch (error) {
    result.available = false;
    result.error = String(error);
  }
  return JSON.stringify(result);
})()
"#
        }
        "keynote" => {
            r#"
(() => {
  const app = Application("Keynote");
  if (!app.running()) return JSON.stringify({ available: false, app: "Keynote", reason: "not_running" });
  const result = { available: true, app: "Keynote", grain: "slide" };
  try {
    const document = app.documents[0];
    result.document = String(document.name());
    const slide = document.currentSlide();
    result.slide = { index: slide.slideNumber(), name: String(slide.name()) };
  } catch (error) {
    result.available = false;
    result.error = String(error);
  }
  return JSON.stringify(result);
})()
"#
        }
        "numbers" => {
            r#"
(() => {
  const app = Application("Numbers");
  if (!app.running()) return JSON.stringify({ available: false, app: "Numbers", reason: "not_running" });
  const result = { available: true, app: "Numbers", grain: "table" };
  try {
    const document = app.documents[0];
    result.document = String(document.name());
    const sheet = document.activeSheet();
    result.sheet = String(sheet.name());
  } catch (error) {
    result.available = false;
    result.error = String(error);
  }
  return JSON.stringify(result);
})()
"#
        }
        "pages" => {
            r#"
(() => {
  const app = Application("Pages");
  if (!app.running()) return JSON.stringify({ available: false, app: "Pages", reason: "not_running" });
  const result = { available: true, app: "Pages", grain: "section" };
  try {
    const document = app.documents[0];
    result.document = String(document.name());
    try { result.selection = String(app.selection()); } catch (_) {}
  } catch (error) {
    result.available = false;
    result.error = String(error);
  }
  return JSON.stringify(result);
})()
"#
        }
        "outlook" => {
            r#"
(() => {
  const app = Application("Microsoft Outlook");
  if (!app.running()) return JSON.stringify({ available: false, app: "Outlook", reason: "not_running" });
  const result = { available: true, app: "Outlook", grain: "thread" };
  try {
    const selected = app.selectedObjects();
    result.selectionCount = selected.length;
    result.items = selected().slice(0, 5).map((item) => {
      const entry = {};
      try { entry.subject = String(item.subject()); } catch (_) {}
      try { entry.sender = String(item.sender().name()); } catch (_) {}
      return entry;
    });
  } catch (error) {
    result.available = false;
    result.error = String(error);
  }
  return JSON.stringify(result);
})()
"#
        }
        "mail" => {
            r#"
(() => {
  const app = Application("Mail");
  if (!app.running()) return JSON.stringify({ available: false, app: "Mail", reason: "not_running" });
  const result = { available: true, app: "Mail", grain: "thread" };
  try {
    const selected = app.selection();
    result.selectionCount = selected.length;
    result.messages = selected().slice(0, 5).map((item) => ({
      subject: String(item.subject()),
      sender: String(item.sender()),
      id: String(item.id())
    }));
  } catch (error) {
    result.available = false;
    result.error = String(error);
  }
  return JSON.stringify(result);
})()
"#
        }
        "calendar" => {
            r#"
(() => {
  const app = Application("Calendar");
  if (!app.running()) return JSON.stringify({ available: false, app: "Calendar", reason: "not_running" });
  return JSON.stringify({ available: true, app: "Calendar", grain: "event", note: "Calendar exposes events by calendar; direct UI selection is limited." });
})()
"#
        }
        "notes" => {
            r#"
(() => {
  const app = Application("Notes");
  if (!app.running()) return JSON.stringify({ available: false, app: "Notes", reason: "not_running" });
  const result = { available: true, app: "Notes", grain: "note" };
  try {
    const account = app.defaultAccount();
    result.account = String(account.name());
    try {
      const note = app.selection()[0];
      result.note = { name: String(note.name()), id: String(note.id()) };
    } catch (_) {}
  } catch (error) {
    result.available = false;
    result.error = String(error);
  }
  return JSON.stringify(result);
})()
"#
        }
        _ => {
            return Err(format!(
                "{app_name} does not have a local selection probe executor yet."
            ));
        }
    };

    Ok(script.to_string())
}

fn show_main_window_inner(app: &AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        return Ok(());
    }
    Err("The main Entropic window is not available.".to_string())
}

fn ensure_companion_window(app: &AppHandle) -> Result<WebviewWindow, String> {
    if let Some(window) = app.get_webview_window(COMPANION_WINDOW_LABEL) {
        return Ok(window);
    }

    WebviewWindowBuilder::new(
        app,
        COMPANION_WINDOW_LABEL,
        WebviewUrl::App("index.html?companion=1".into()),
    )
    .title("Entropic Companion")
    .inner_size(460.0, 700.0)
    .min_inner_size(360.0, 520.0)
    .resizable(true)
    .decorations(false)
    .always_on_top(true)
    .visible_on_all_workspaces(true)
    .shadow(true)
    .focused(true)
    .visible(false)
    .build()
    .map_err(|error| format!("Failed to create Companion window: {error}"))
}

fn show_companion_window_inner(app: &AppHandle) -> Result<(), String> {
    let window = ensure_companion_window(app)?;
    let _ = window.unminimize();
    window
        .set_always_on_top(true)
        .map_err(|error| format!("Failed to keep Companion above host apps: {error}"))?;
    let _ = window.set_visible_on_all_workspaces(true);
    window
        .show()
        .map_err(|error| format!("Failed to show Companion window: {error}"))?;
    window
        .set_focus()
        .map_err(|error| format!("Failed to focus Companion window: {error}"))
}

fn hide_companion_window_inner(app: &AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(COMPANION_WINDOW_LABEL) {
        window
            .hide()
            .map_err(|error| format!("Failed to hide Companion window: {error}"))?;
    }
    Ok(())
}

fn toggle_companion_window_inner(app: &AppHandle) -> Result<(), String> {
    let window = ensure_companion_window(app)?;
    if window.is_visible().unwrap_or(false) {
        window
            .hide()
            .map_err(|error| format!("Failed to hide Companion window: {error}"))?;
        return Ok(());
    }

    let _ = window.unminimize();
    let _ = window.set_always_on_top(true);
    let _ = window.set_visible_on_all_workspaces(true);
    window
        .show()
        .map_err(|error| format!("Failed to show Companion window: {error}"))?;
    window
        .set_focus()
        .map_err(|error| format!("Failed to focus Companion window: {error}"))
}

fn toggle_companion_window_or_fallback(app: &AppHandle) {
    if let Err(error) = toggle_companion_window_inner(app) {
        eprintln!("[Entropic] {error}; falling back to in-app Companion overlay");
        let _ = show_main_window_inner(app);
        let _ = app.emit(COMPANION_TOGGLE_REQUESTED_EVENT, ());
    }
}

#[cfg(target_os = "macos")]
fn install_companion_tray(app: &AppHandle) -> Result<(), String> {
    use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

    let icon = app.default_window_icon().cloned().ok_or_else(|| {
        "Entropic app icon is unavailable for the Companion menu bar item.".to_string()
    })?;
    let app_handle = app.clone();
    TrayIconBuilder::with_id("companion")
        .tooltip("Entropic Companion")
        .icon(icon)
        .on_tray_icon_event(move |_tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                toggle_companion_window_or_fallback(&app_handle);
            }
        })
        .build(app)
        .map(|_| ())
        .map_err(|error| format!("Failed to install Companion menu bar item: {error}"))
}

#[cfg(not(target_os = "macos"))]
fn install_companion_tray(_app: &AppHandle) -> Result<(), String> {
    Ok(())
}

#[cfg(target_os = "macos")]
fn install_companion_hotkey(app: &AppHandle) -> Result<(), String> {
    macos::install_global_hotkey(app)
}

#[cfg(not(target_os = "macos"))]
fn install_companion_hotkey(_app: &AppHandle) -> Result<(), String> {
    Ok(())
}

#[cfg(target_os = "macos")]
fn install_focus_listener(app: &AppHandle) -> Result<(), String> {
    macos::install_focus_listener(app)
}

#[cfg(not(target_os = "macos"))]
fn install_focus_listener(_app: &AppHandle) -> Result<(), String> {
    Ok(())
}

#[cfg(target_os = "macos")]
fn frontmost_focus_snapshot() -> Option<CompanionFocusSnapshot> {
    macos::frontmost_focus_snapshot()
}

#[cfg(not(target_os = "macos"))]
fn frontmost_focus_snapshot() -> Option<CompanionFocusSnapshot> {
    None
}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use objc::declare::ClassDecl;
    use objc::runtime::{Class, Object, Sel};
    use objc::{class, msg_send, sel, sel_impl};
    use std::ffi::CStr;
    use std::os::raw::{c_char, c_void};
    use std::ptr;
    use std::sync::OnceLock;

    #[link(name = "AppKit", kind = "framework")]
    unsafe extern "C" {}

    #[link(name = "Carbon", kind = "framework")]
    unsafe extern "C" {
        fn GetApplicationEventTarget() -> EventTargetRef;
        fn InstallEventHandler(
            in_target: EventTargetRef,
            in_handler: EventHandlerUPP,
            in_num_types: u32,
            in_list: *const EventTypeSpec,
            in_user_data: *mut c_void,
            out_ref: *mut EventHandlerRef,
        ) -> i32;
        fn RegisterEventHotKey(
            in_hot_key_code: u32,
            in_hot_key_modifiers: u32,
            in_hot_key_id: EventHotKeyID,
            in_target: EventTargetRef,
            in_options: u32,
            out_ref: *mut EventHotKeyRef,
        ) -> i32;
    }

    type EventTargetRef = *mut c_void;
    type EventHandlerRef = *mut c_void;
    type EventRef = *mut c_void;
    type EventHandlerCallRef = *mut c_void;
    type EventHotKeyRef = *mut c_void;
    type EventHandlerUPP =
        Option<unsafe extern "C" fn(EventHandlerCallRef, EventRef, *mut c_void) -> i32>;

    #[repr(C)]
    struct EventTypeSpec {
        event_class: u32,
        event_kind: u32,
    }

    #[repr(C)]
    struct EventHotKeyID {
        signature: u32,
        id: u32,
    }

    const K_EVENT_CLASS_KEYBOARD: u32 = 0x6B65_7962;
    const K_EVENT_HOT_KEY_PRESSED: u32 = 5;
    const CMD_KEY: u32 = 1 << 8;
    const SHIFT_KEY: u32 = 1 << 9;
    const KEY_CODE_E: u32 = 14;
    const HOTKEY_SIGNATURE: u32 = 0x456E_7472;
    static FOCUS_OBSERVER: OnceLock<usize> = OnceLock::new();
    static HOTKEY_REF: OnceLock<usize> = OnceLock::new();
    static HOTKEY_HANDLER_REF: OnceLock<usize> = OnceLock::new();

    fn nsstring(value: &str) -> *mut Object {
        unsafe {
            let ns_string: *mut Object = msg_send![class!(NSString), alloc];
            let ns_string: *mut Object = msg_send![ns_string, initWithBytes: value.as_ptr()
                length: value.len()
                encoding: 4usize];
            ns_string
        }
    }

    fn nsstring_to_string(value: *mut Object) -> Option<String> {
        if value.is_null() {
            return None;
        }
        unsafe {
            let c_string: *const c_char = msg_send![value, UTF8String];
            if c_string.is_null() {
                return None;
            }
            CStr::from_ptr(c_string)
                .to_str()
                .ok()
                .map(|s| s.to_string())
        }
    }

    pub fn frontmost_focus_snapshot() -> Option<CompanionFocusSnapshot> {
        unsafe {
            let workspace: *mut Object = msg_send![class!(NSWorkspace), sharedWorkspace];
            if workspace.is_null() {
                return None;
            }
            let app: *mut Object = msg_send![workspace, frontmostApplication];
            if app.is_null() {
                return None;
            }

            let bundle_id_obj: *mut Object = msg_send![app, bundleIdentifier];
            let app_name_obj: *mut Object = msg_send![app, localizedName];
            let pid: i32 = msg_send![app, processIdentifier];
            let bundle_id = nsstring_to_string(bundle_id_obj);
            let app_name = nsstring_to_string(app_name_obj);
            let window_title = frontmost_window_title_from_system_events();

            Some(CompanionFocusSnapshot {
                supported: true,
                bundle_id,
                app_name,
                window_title,
                pid: Some(pid),
                focused_at_ms: now_ms(),
            })
        }
    }

    fn frontmost_window_title_from_system_events() -> Option<String> {
        if which::which("osascript").is_err() {
            return None;
        }

        let script = r#"
(() => {
  try {
    const systemEvents = Application("System Events");
    const processes = systemEvents.applicationProcesses.whose({ frontmost: true })();
    if (!processes.length) return "";
    const windows = processes[0].windows();
    if (!windows.length) return "";
    return String(windows[0].name());
  } catch (error) {
    return "";
  }
})()
"#;
        let output = Command::new("osascript")
            .arg("-l")
            .arg("JavaScript")
            .arg("-e")
            .arg(script)
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let title = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if title.is_empty() {
            None
        } else {
            Some(title)
        }
    }

    extern "C" fn workspace_did_activate(_this: &Object, _cmd: Sel, _notification: *mut Object) {
        let Some(app) = COMPANION_APP_HANDLE.get() else {
            return;
        };
        if let Some(snapshot) = frontmost_focus_snapshot() {
            update_focus_cache(app, snapshot);
        }
    }

    fn focus_observer_class() -> &'static Class {
        if let Some(class) = Class::get("EntropicCompanionFocusObserver") {
            return class;
        }

        let superclass = class!(NSObject);
        let mut decl = ClassDecl::new("EntropicCompanionFocusObserver", superclass)
            .expect("failed to declare focus observer class");
        unsafe {
            decl.add_method(
                sel!(workspaceDidActivateApplication:),
                workspace_did_activate as extern "C" fn(&Object, Sel, *mut Object),
            );
        }
        decl.register()
    }

    pub fn install_focus_listener(app: &AppHandle) -> Result<(), String> {
        if FOCUS_OBSERVER.get().is_some() {
            return Ok(());
        }
        let _ = COMPANION_APP_HANDLE.set(app.clone());

        unsafe {
            let observer: *mut Object = msg_send![focus_observer_class(), new];
            if observer.is_null() {
                return Err("Failed to create Companion focus observer.".to_string());
            }

            let workspace: *mut Object = msg_send![class!(NSWorkspace), sharedWorkspace];
            let center: *mut Object = msg_send![workspace, notificationCenter];
            let name = nsstring("NSWorkspaceDidActivateApplicationNotification");
            let _: () = msg_send![center,
                addObserver: observer
                selector: sel!(workspaceDidActivateApplication:)
                name: name
                object: ptr::null_mut::<Object>()
            ];
            let _ = FOCUS_OBSERVER.set(observer as usize);
        }

        Ok(())
    }

    unsafe extern "C" fn hotkey_handler(
        _next_handler: EventHandlerCallRef,
        _event: EventRef,
        _user_data: *mut c_void,
    ) -> i32 {
        if let Some(app) = COMPANION_APP_HANDLE.get() {
            toggle_companion_window_or_fallback(app);
        }
        0
    }

    pub fn install_global_hotkey(app: &AppHandle) -> Result<(), String> {
        if HOTKEY_REF.get().is_some() {
            return Ok(());
        }
        let _ = COMPANION_APP_HANDLE.set(app.clone());

        unsafe {
            let target = GetApplicationEventTarget();
            if target.is_null() {
                return Err("Failed to resolve macOS application event target.".to_string());
            }

            let event_type = EventTypeSpec {
                event_class: K_EVENT_CLASS_KEYBOARD,
                event_kind: K_EVENT_HOT_KEY_PRESSED,
            };
            let mut handler_ref: EventHandlerRef = ptr::null_mut();
            let handler_status = InstallEventHandler(
                target,
                Some(hotkey_handler),
                1,
                &event_type,
                ptr::null_mut(),
                &mut handler_ref,
            );
            if handler_status != 0 {
                return Err(format!(
                    "Failed to install Companion global hotkey handler: OSStatus {handler_status}"
                ));
            }
            let _ = HOTKEY_HANDLER_REF.set(handler_ref as usize);

            let mut hotkey_ref: EventHotKeyRef = ptr::null_mut();
            let hotkey_id = EventHotKeyID {
                signature: HOTKEY_SIGNATURE,
                id: 1,
            };
            let hotkey_status = RegisterEventHotKey(
                KEY_CODE_E,
                CMD_KEY | SHIFT_KEY,
                hotkey_id,
                target,
                0,
                &mut hotkey_ref,
            );
            if hotkey_status != 0 {
                return Err(format!(
                    "Failed to register Companion global hotkey: OSStatus {hotkey_status}"
                ));
            }
            let _ = HOTKEY_REF.set(hotkey_ref as usize);
        }

        Ok(())
    }
}
