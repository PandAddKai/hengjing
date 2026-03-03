use std::collections::VecDeque;
use std::sync::{Arc, OnceLock};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use tokio::sync::{oneshot, broadcast, Mutex, mpsc};
use axum::{
    Router,
    body::Body,
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    extract::{Query, State},
    http::{header, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::get,
};
use rust_embed::RustEmbed;
use anyhow::Result;

use crate::config::load_standalone_config;
use crate::mcp::types::PopupRequest;
use crate::log_important;

const DEFAULT_PORT: u16 = 18963;
const MAX_INTERACTION_HISTORY: usize = 5;
const AUTH_TOKEN_LENGTH: usize = 32;

/// 嵌入编译好的 Vue 前端
#[derive(RustEmbed)]
#[folder = "dist"]
struct WebAssets;

/// 交互记录
#[derive(Clone, serde::Serialize)]
struct InteractionRecord {
    id: String,
    message: String,
    response: String,
    timestamp: String,
    is_markdown: bool,
}

/// Web 服务器共享状态
struct WebState {
    current_request: Mutex<Option<PopupRequest>>,
    current_response_tx: Mutex<Option<oneshot::Sender<String>>>,
    config_json: String,
    interaction_history: Mutex<VecDeque<InteractionRecord>>,
    /// 广播通道：向所有 WebSocket 客户端推送事件
    event_tx: broadcast::Sender<String>,
    /// 认证 token（启动时随机生成）
    auth_token: String,
    /// 活跃 WebSocket 连接数（用于防止多客户端响应冲突）
    active_ws_count: AtomicUsize,
}

/// 生成随机认证 token
fn generate_auth_token() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let mut result = String::with_capacity(AUTH_TOKEN_LENGTH);
    let chars: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    for _ in 0..AUTH_TOKEN_LENGTH {
        let s = RandomState::new();
        let mut hasher = s.build_hasher();
        hasher.write_usize(result.len());
        let idx = (hasher.finish() as usize) % chars.len();
        result.push(chars[idx] as char);
    }
    result
}

/// 从 query string 中提取 token 参数
#[derive(serde::Deserialize)]
struct AuthQuery {
    #[serde(default)]
    token: String,
}

/// 全局持久化服务器实例
struct ServerInstance {
    state: Arc<WebState>,
    is_running: AtomicBool,
}

static SERVER_INSTANCE: OnceLock<ServerInstance> = OnceLock::new();

/// 检测是否应该使用 Web 模式
pub fn should_use_web_mode() -> bool {
    // 强制 Web 模式（环境变量）
    if std::env::var("HENGJING_WEB_MODE").unwrap_or_default() == "1" {
        return true;
    }

    // Linux: 检查 $DISPLAY 和 $WAYLAND_DISPLAY
    #[cfg(target_os = "linux")]
    {
        let has_display = std::env::var("DISPLAY").is_ok();
        let has_wayland = std::env::var("WAYLAND_DISPLAY").is_ok();
        if !has_display && !has_wayland {
            return true;
        }
    }

    false
}

/// 启动 Web 模式处理 MCP 请求（持久化服务器）
pub async fn handle_web_mode(request: &PopupRequest) -> Result<String> {
    let (response_tx, response_rx) = oneshot::channel::<String>();

    let instance = SERVER_INSTANCE.get_or_init(|| {
        let (event_tx, _) = broadcast::channel(100);
        let config_json = build_config_json();
        let auth_token = generate_auth_token();

        ServerInstance {
            state: Arc::new(WebState {
                current_request: Mutex::new(None),
                current_response_tx: Mutex::new(None),
                config_json,
                interaction_history: Mutex::new(VecDeque::new()),
                event_tx,
                auth_token,
                active_ws_count: AtomicUsize::new(0),
            }),
            is_running: AtomicBool::new(false),
        }
    });

    // 启动服务器（仅首次）
    if !instance.is_running.swap(true, Ordering::SeqCst) {
        match try_bind_listener().await {
            Ok(listener) => {
                let local_addr = listener.local_addr()
                    .map_err(|e| anyhow::anyhow!("获取监听地址失败: {}", e))?;
                let token = &instance.state.auth_token;
                let url_with_token = format!("http://{}?token={}", local_addr, token);

                log_important!(info, "Web 模式已启动: {}", url_with_token);
                eprintln!("\n================================================");
                eprintln!("  且慢 Web UI: {}", url_with_token);
                eprintln!("  在浏览器中打开上述地址进行交互");
                eprintln!("  （URL 包含认证 token，请勿泄露）");
                eprintln!("================================================\n");

                let _ = open::that(&url_with_token);

                let state = instance.state.clone();
                tokio::spawn(async move {
                    if let Err(e) = run_web_server(state, listener).await {
                        log_important!(error, "Web 服务器运行错误: {}", e);
                    }
                });
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
            Err(e) => {
                instance.is_running.store(false, Ordering::SeqCst);
                return Err(anyhow::anyhow!("Web 服务器启动失败（端口可能被占用）: {}", e));
            }
        }
    }

    // 设置当前请求和响应通道
    *instance.state.current_request.lock().await = Some(request.clone());
    *instance.state.current_response_tx.lock().await = Some(response_tx);

    // 向已连接的 WebSocket 客户端推送 mcp-request 事件
    let event = build_mcp_request_event(request);
    let _ = instance.state.event_tx.send(event);

    // 等待用户响应
    match response_rx.await {
        Ok(resp) => {
            log_important!(info, "Web 模式收到用户响应");

            // 提取用户输入用于历史记录显示
            let display_response = if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&resp) {
                parsed.get("user_input")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&resp)
                    .to_string()
            } else {
                resp.clone()
            };

            // 存储交互记录
            let mut history = instance.state.interaction_history.lock().await;
            if history.len() >= MAX_INTERACTION_HISTORY {
                history.pop_front();
            }
            history.push_back(InteractionRecord {
                id: request.id.clone(),
                message: request.message.clone(),
                response: display_response,
                timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
                is_markdown: request.is_markdown,
            });

            // 清除当前请求
            *instance.state.current_request.lock().await = None;

            Ok(resp)
        }
        Err(_) => Err(anyhow::anyhow!("响应通道已关闭"))
    }
}

/// 构建 mcp-request 事件 JSON
fn build_mcp_request_event(request: &PopupRequest) -> String {
    serde_json::json!({
        "type": "event",
        "event": "mcp-request",
        "payload": {
            "id": request.id,
            "message": request.message,
            "predefined_options": request.predefined_options,
            "is_markdown": request.is_markdown,
        }
    }).to_string()
}

/// 尝试绑定监听端口，支持端口冲突时自动重试
async fn try_bind_listener() -> Result<tokio::net::TcpListener> {
    let host = std::env::var("HENGJING_WEB_HOST")
        .unwrap_or_else(|_| "127.0.0.1".to_string());

    let base_port = std::env::var("HENGJING_WEB_PORT")
        .ok()
        .and_then(|p| p.parse::<u16>().ok())
        .unwrap_or(DEFAULT_PORT);

    const MAX_PORT_RETRIES: u16 = 10;
    for offset in 0..MAX_PORT_RETRIES {
        let port = base_port + offset;
        let addr = format!("{}:{}", host, port);
        match tokio::net::TcpListener::bind(&addr).await {
            Ok(listener) => {
                if offset > 0 {
                    log_important!(warn, "默认端口 {} 已被占用，使用备用端口 {}", base_port, port);
                }
                return Ok(listener);
            }
            Err(e) if offset < MAX_PORT_RETRIES - 1 => {
                log_important!(warn, "端口 {} 绑定失败: {}，尝试下一个端口", port, e);
                continue;
            }
            Err(e) => {
                return Err(anyhow::anyhow!(
                    "无法绑定端口 {}-{}，全部被占用: {}",
                    base_port, base_port + MAX_PORT_RETRIES - 1, e
                ));
            }
        }
    }
    unreachable!()
}

/// 启动 HTTP + WebSocket 服务器（使用已绑定的 listener）
async fn run_web_server(state: Arc<WebState>, listener: tokio::net::TcpListener) -> Result<()> {
    let app = Router::new()
        .route("/ws", get(ws_handler))
        .route("/", get(serve_index))
        .fallback(get(serve_static_fallback))
        .with_state(state.clone());

    axum::serve(listener, app).await?;

    Ok(())
}

/// 提供 index.html（注入 Tauri shim，需要 token 认证）
async fn serve_index(
    Query(query): Query<AuthQuery>,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    if query.token != state.auth_token {
        return (StatusCode::FORBIDDEN, "Invalid or missing auth token").into_response();
    }

    match WebAssets::get("index.html") {
        Some(content) => {
            let html = String::from_utf8_lossy(&content.data);
            let request = state.current_request.lock().await;
            let req_ref = request.as_ref();
            let shim = build_tauri_shim(req_ref, &state.config_json, &state.auth_token);
            let modified = html.replacen(
                "<script ",
                &format!("<script>{}</script>\n<script ", shim),
                1,
            );
            Html(modified).into_response()
        }
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

/// 提供静态文件（JS/CSS/字体/图片）- fallback handler
async fn serve_static_fallback(uri: axum::http::Uri) -> impl IntoResponse {
    let path = uri.path().trim_start_matches('/');
    match WebAssets::get(path) {
        Some(content) => {
            let mime = mime_guess::from_path(&path)
                .first_or_octet_stream()
                .to_string();
            Response::builder()
                .header(header::CONTENT_TYPE, mime)
                .body(Body::from(content.data.to_vec()))
                .unwrap()
                .into_response()
        }
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

/// WebSocket 升级处理（需要 token 认证）
async fn ws_handler(
    Query(query): Query<AuthQuery>,
    ws: WebSocketUpgrade,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    if query.token != state.auth_token {
        return (StatusCode::FORBIDDEN, "Invalid or missing auth token").into_response();
    }
    ws.on_upgrade(move |socket| handle_ws(socket, state)).into_response()
}

/// WebSocket 连接处理（支持双向通信，带连接计数）
async fn handle_ws(mut socket: WebSocket, state: Arc<WebState>) {
    let conn_count = state.active_ws_count.fetch_add(1, Ordering::SeqCst) + 1;
    if conn_count > 1 {
        log_important!(warn, "检测到多个 WebSocket 客户端（当前 {} 个），只有最后提交的响应会被采纳", conn_count);
    }

    let mut event_rx = state.event_tx.subscribe();
    let (outgoing_tx, mut outgoing_rx) = mpsc::unbounded_channel::<String>();

    // 转发广播事件到 outgoing 通道
    let event_fwd_tx = outgoing_tx.clone();
    let event_forwarder = tokio::spawn(async move {
        loop {
            match event_rx.recv().await {
                Ok(event_str) => {
                    if event_fwd_tx.send(event_str).is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            }
        }
    });

    // 发送当前请求（首次连接或页面刷新时）
    if let Some(req) = state.current_request.lock().await.as_ref() {
        let event = build_mcp_request_event(req);
        let _ = socket.send(Message::Text(event.into())).await;
    }

    // 主循环：交替处理 outgoing 消息和 WebSocket 消息
    loop {
        // 先排空待发送的 outgoing 消息
        while let Ok(msg) = outgoing_rx.try_recv() {
            if socket.send(Message::Text(msg.into())).await.is_err() {
                event_forwarder.abort();
                state.active_ws_count.fetch_sub(1, Ordering::SeqCst);
                return;
            }
        }

        // 等待 WebSocket 消息（带超时，以便定期检查 outgoing）
        match tokio::time::timeout(
            std::time::Duration::from_millis(100),
            socket.recv()
        ).await {
            Ok(Some(Ok(Message::Text(text)))) => {
                if let Some(response) = process_ws_message(&text, &state).await {
                    if socket.send(Message::Text(response.into())).await.is_err() {
                        break;
                    }
                }
            }
            Ok(Some(Ok(Message::Close(_)))) | Ok(Some(Err(_))) | Ok(None) => break,
            Ok(Some(Ok(_))) => {} // 忽略非文本消息
            Err(_) => {} // 超时，继续循环检查 outgoing
        }
    }

    event_forwarder.abort();
    state.active_ws_count.fetch_sub(1, Ordering::SeqCst);
}

/// 处理 WebSocket 消息，返回可选的响应
async fn process_ws_message(text: &str, state: &WebState) -> Option<String> {
    let parsed = serde_json::from_str::<serde_json::Value>(text).ok()?;
    let msg_type = parsed.get("type").and_then(|t| t.as_str()).unwrap_or("");

    match msg_type {
        // 客户端准备就绪
        "ready" => {
            if let Some(req) = state.current_request.lock().await.as_ref() {
                Some(build_mcp_request_event(req))
            } else {
                None
            }
        }

        // invoke 调用
        "invoke" => {
            let cmd = parsed.get("cmd").and_then(|c| c.as_str()).unwrap_or("");
            let args = parsed.get("args").cloned().unwrap_or(serde_json::json!({}));
            let call_id = parsed.get("id").and_then(|i| i.as_u64()).unwrap_or(0);

            let result = handle_invoke(cmd, &args, state).await;

            let response = serde_json::json!({
                "type": "invoke_result",
                "id": call_id,
                "result": result,
            });
            Some(response.to_string())
        }

        _ => None,
    }
}

/// 处理 invoke 命令
async fn handle_invoke(cmd: &str, args: &serde_json::Value, state: &WebState) -> serde_json::Value {
    // Web 服务器专属命令（需要访问运行时 state）
    match cmd {
        "get_cli_args" => {
            return serde_json::json!({"mcp_request": "__web_mode__"});
        }
        "read_mcp_request" => {
            return if let Some(req) = state.current_request.lock().await.as_ref() {
                serde_json::json!({
                    "id": req.id,
                    "message": req.message,
                    "predefined_options": req.predefined_options,
                    "is_markdown": req.is_markdown,
                })
            } else {
                serde_json::json!(null)
            };
        }
        "send_mcp_response" => {
            if let Some(response) = args.get("response") {
                let response_str = if response.is_string() {
                    response.as_str().unwrap().to_string()
                } else {
                    response.to_string()
                };
                let mut tx_lock = state.current_response_tx.lock().await;
                if let Some(tx) = tx_lock.take() {
                    let _ = tx.send(response_str);
                }
            }
            return serde_json::json!(null);
        }
        "get_interaction_history" => {
            let history = state.interaction_history.lock().await;
            return serde_json::to_value(&*history).unwrap_or(serde_json::json!([]));
        }
        "exit_app" => {
            return serde_json::json!(null);
        }
        _ => {}
    }

    // 委托给统一的命令分发器（配置读写 + GUI 默认值）
    if let Some(result) = super::commands::dispatch(cmd, args) {
        return result;
    }

    log_important!(warn, "Web 模式未处理的 invoke: {}", cmd);
    serde_json::json!(null)
}

/// 构建 Tauri API shim（注入到 index.html 的 JS）
fn build_tauri_shim(request: Option<&PopupRequest>, _config_json: &str, auth_token: &str) -> String {
    let request_json = request
        .map(|r| serde_json::to_string(r).unwrap_or_default())
        .unwrap_or_else(|| "null".to_string());

    format!(r#"
/* 且慢 Web Mode - Tauri API Shim */
(function() {{
  'use strict';

  window.__HENGJING_WEB_BUILD__ = 1;
  var _authToken = '{auth_token}';

  // === Callback & Event Infrastructure ===
  var callbackId = 0;
  var callbacks = {{}};
  var eventHandlers = {{}};   // eventName -> [{{ id, handler }}]
  var listenerId = 0;

  function transformCallback(fn, once) {{
    var id = ++callbackId;
    callbacks['_' + id] = function() {{
      if (once) delete callbacks['_' + id];
      return fn.apply(null, arguments);
    }};
    return id;
  }}

  // Called by shim to dispatch events (same interface Tauri uses)
  function dispatchTauriEvent(eventName, payload) {{
    var handlers = eventHandlers[eventName] || [];
    handlers.forEach(function(entry) {{
      try {{ entry.handler({{ payload: payload, id: entry.id }}); }} catch(e) {{ console.error(e); }}
    }});
  }}

  // === WebSocket ===
  var ws = null, wsReady = false;
  var pendingInvokes = {{}};
  var invokeIdCounter = 0;

  function connectWebSocket() {{
    var protocol = location.protocol === 'https:' ? 'wss:' : 'ws:';
    ws = new WebSocket(protocol + '//' + location.host + '/ws?token=' + encodeURIComponent(_authToken));
    ws.onopen = function() {{
      wsReady = true;
      ws.send(JSON.stringify({{ type: 'ready' }}));
    }};
    ws.onmessage = function(event) {{
      try {{
        var msg = JSON.parse(event.data);
        if (msg.type === 'invoke_result' && pendingInvokes[msg.id]) {{
          pendingInvokes[msg.id].resolve(msg.result);
          delete pendingInvokes[msg.id];
        }} else if (msg.type === 'event') {{
          dispatchTauriEvent(msg.event, msg.payload);
        }}
      }} catch(e) {{}}
    }};
    ws.onclose = function() {{ wsReady = false; setTimeout(connectWebSocket, 3000); }};
  }}

  function wsInvoke(cmd, args) {{
    return new Promise(function(resolve) {{
      if (!ws || !wsReady) {{ resolve(localInvoke(cmd, args)); return; }}
      var id = ++invokeIdCounter;
      pendingInvokes[id] = {{ resolve: resolve }};
      ws.send(JSON.stringify({{ type: 'invoke', id: id, cmd: cmd, args: args || {{}} }}));
      setTimeout(function() {{
        if (pendingInvokes[id]) {{ delete pendingInvokes[id]; resolve(localInvoke(cmd, args)); }}
      }}, 30000);
    }});
  }}

  var _req = {request_json};
  function localInvoke(cmd, args) {{
    switch(cmd) {{
      case 'get_cli_args': return {{ mcp_request: '__web_mode__' }};
      case 'read_mcp_request': return _req;
      case 'play_notification_sound': return null;
      case 'exit_app': return null;
      case 'get_telegram_config': return {{ enabled: false, hide_frontend_popup: false }};
      default: return null;
    }}
  }}

  // === Mock __TAURI_INTERNALS__ ===
  window.__TAURI_INTERNALS__ = {{
    transformCallback: transformCallback,
    invoke: function(cmd, args, options) {{
      // Handle event plugin commands locally
      if (cmd === 'plugin:event|listen') {{
        var evtName = args && args.event;
        var handlerCbId = args && args.handler;
        if (evtName && handlerCbId != null) {{
          var id = ++listenerId;
          var handlerFn = callbacks['_' + handlerCbId];
          if (!eventHandlers[evtName]) eventHandlers[evtName] = [];
          eventHandlers[evtName].push({{ id: id, handler: handlerFn || function() {{}} }});
          return Promise.resolve(id);
        }}
        return Promise.resolve(0);
      }}
      if (cmd === 'plugin:event|unlisten') {{
        var evtName2 = args && args.event;
        var evtId = args && args.eventId;
        if (evtName2 && eventHandlers[evtName2]) {{
          eventHandlers[evtName2] = eventHandlers[evtName2].filter(function(e) {{ return e.id !== evtId; }});
        }}
        return Promise.resolve();
      }}
      if (cmd === 'plugin:event|emit') {{
        dispatchTauriEvent(args && args.event, args && args.payload);
        return Promise.resolve();
      }}
      // Route all other commands through WebSocket
      return wsInvoke(cmd, args);
    }},
    convertFileSrc: function(path) {{ return path; }},
    metadata: {{
      currentWindow: {{ label: 'main' }},
      currentWebview: {{ label: 'main' }},
      windows: ['main'],
      webviews: ['main']
    }},
    plugins: {{}}
  }};

  connectWebSocket();
  console.log('[且慢 Web] Tauri API Shim 已加载');
}})();
"#)
}

/// 构建配置 JSON（供 WebSocket invoke 使用）
fn build_config_json() -> String {
    let config = load_standalone_config().unwrap_or_default();
    serde_json::to_string(&config).unwrap_or_default()
}
