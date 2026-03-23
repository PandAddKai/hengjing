use anyhow::Result;
use axum::{
    body::Body,
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    extract::{Query, State},
    http::{header, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::get,
    Router,
};
use rust_embed::RustEmbed;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use tokio::sync::{broadcast, mpsc, oneshot, Mutex};

use crate::config::load_standalone_config;
use crate::constants::telegram as telegram_constants;
use crate::log_important;
use crate::mcp::types::PopupRequest;

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
    /// 等待响应的请求（按 request_id 索引）
    pending_requests: Mutex<HashMap<String, (PopupRequest, oneshot::Sender<String>)>>,
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
                pending_requests: Mutex::new(HashMap::new()),
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
                let local_addr = listener
                    .local_addr()
                    .map_err(|e| anyhow::anyhow!("获取监听地址失败: {}", e))?;
                let token = &instance.state.auth_token;
                let url_with_token = format!("http://{}?token={}", local_addr, token);

                log_important!(info, "Web 模式已启动: {}", url_with_token);
                eprintln!("\n================================================");
                eprintln!("  恒境 Web UI: {}", url_with_token);
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
                return Err(anyhow::anyhow!(
                    "Web 服务器启动失败（端口可能被占用）: {}",
                    e
                ));
            }
        }
    }

    // 将请求插入 HashMap，等待对应的 oneshot 响应
    {
        let mut pending = instance.state.pending_requests.lock().await;
        pending.insert(request.id.clone(), (request.clone(), response_tx));
    }

    // 向已连接的 WebSocket 客户端推送 mcp-request 事件
    let event = build_mcp_request_event(request);
    let _ = instance.state.event_tx.send(event);

    // 等待用户响应
    match response_rx.await {
        Ok(resp) => {
            log_important!(info, "Web 模式收到用户响应");

            // 提取用户输入用于历史记录显示
            let display_response =
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&resp) {
                    parsed
                        .get("user_input")
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

            Ok(resp)
        }
        Err(_) => Err(anyhow::anyhow!("响应通道已关闭")),
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
    })
    .to_string()
}

/// 尝试绑定监听端口，支持端口冲突时自动重试
async fn try_bind_listener() -> Result<tokio::net::TcpListener> {
    let host = std::env::var("HENGJING_WEB_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());

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
                    log_important!(
                        warn,
                        "默认端口 {} 已被占用，使用备用端口 {}",
                        base_port,
                        port
                    );
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
                    base_port,
                    base_port + MAX_PORT_RETRIES - 1,
                    e
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
            let pending = state.pending_requests.lock().await;
            let first_request = pending.values().next().map(|(req, _)| req);
            let shim = build_tauri_shim(first_request, &state.config_json, &state.auth_token);
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
    ws.on_upgrade(move |socket| handle_ws(socket, state))
        .into_response()
}

/// WebSocket 连接处理（支持双向通信，带连接计数）
async fn handle_ws(mut socket: WebSocket, state: Arc<WebState>) {
    let conn_count = state.active_ws_count.fetch_add(1, Ordering::SeqCst) + 1;
    if conn_count > 1 {
        log_important!(
            warn,
            "检测到多个 WebSocket 客户端（当前 {} 个），只有最后提交的响应会被采纳",
            conn_count
        );
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

    // 发送所有待处理请求（首次连接或页面刷新时）
    {
        let pending = state.pending_requests.lock().await;
        for (req, _) in pending.values() {
            let event = build_mcp_request_event(req);
            if socket.send(Message::Text(event.into())).await.is_err() {
                event_forwarder.abort();
                state.active_ws_count.fetch_sub(1, Ordering::SeqCst);
                return;
            }
        }
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
        match tokio::time::timeout(std::time::Duration::from_millis(100), socket.recv()).await {
            Ok(Some(Ok(Message::Text(text)))) => {
                if let Some(response) = process_ws_message(&text, &state).await {
                    if socket.send(Message::Text(response.into())).await.is_err() {
                        break;
                    }
                }
            }
            Ok(Some(Ok(Message::Close(_)))) | Ok(Some(Err(_))) | Ok(None) => break,
            Ok(Some(Ok(_))) => {} // 忽略非文本消息
            Err(_) => {}          // 超时，继续循环检查 outgoing
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
        // 客户端准备就绪：所有待处理请求已在 WebSocket 连接时发送
        "ready" => None,

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
            let pending = state.pending_requests.lock().await;
            return if let Some((req, _)) = pending.values().next() {
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

                // 从 args 中提取 request_id 来精确路由
                let request_id = args
                    .get("requestId")
                    .or(args.get("request_id"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                let mut pending = state.pending_requests.lock().await;

                // 按 request_id 查找，或者回退到第一个请求
                let target_id = if let Some(ref id) = request_id {
                    if pending.contains_key(id) {
                        Some(id.clone())
                    } else {
                        pending.keys().next().cloned()
                    }
                } else {
                    pending.keys().next().cloned()
                };

                if let Some(id) = target_id {
                    // 保存对话记录到持久化历史
                    if let Some((req, _)) = pending.get(&id) {
                        let display_response = if let Ok(parsed) =
                            serde_json::from_str::<serde_json::Value>(&response_str)
                        {
                            parsed
                                .get("user_input")
                                .and_then(|v| v.as_str())
                                .unwrap_or(&response_str)
                                .to_string()
                        } else {
                            response_str.clone()
                        };

                        let record = crate::config::history::ConversationRecord {
                            id: req.id.clone(),
                            request_message: req.message.clone(),
                            request_options: req.predefined_options.clone().unwrap_or_default(),
                            response_text: display_response,
                            selected_options: vec![],
                            timestamp: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
                            source: "popup".to_string(),
                        };
                        if let Err(e) = crate::config::history::append_record(record) {
                            log_important!(error, "保存对话记录失败: {}", e);
                        }
                    }

                    // 取出并发送响应
                    if let Some((_, tx)) = pending.remove(&id) {
                        let _ = tx.send(response_str);
                    }
                }
            }
            return serde_json::json!(null);
        }
        "get_interaction_history" => {
            let history = crate::config::history::load_history();
            return serde_json::to_value(&history.records).unwrap_or(serde_json::json!([]));
        }
        "play_notification_sound" => {
            // 通过 WebSocket broadcast 发送音频通知事件到浏览器
            let _ = state.event_tx.send(
                serde_json::json!({
                    "type": "event",
                    "event": "audio-notification",
                    "payload": {}
                })
                .to_string(),
            );
            return serde_json::json!(null);
        }
        "exit_app" => {
            return serde_json::json!(null);
        }
        "test_telegram_connection_cmd" => {
            let bot_token = args
                .get("bot_token")
                .or(args.get("botToken"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let chat_id = args
                .get("chat_id")
                .or(args.get("chatId"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            // 读取 API URL 配置
            let api_url = load_standalone_config()
                .ok()
                .map(|c| c.telegram_config.api_base_url)
                .filter(|url| url != telegram_constants::API_BASE_URL);
            let api_url_ref = api_url.as_deref();

            match crate::telegram::core::test_telegram_connection_with_api_url(
                &bot_token,
                &chat_id,
                api_url_ref,
            )
            .await
            {
                Ok(msg) => return serde_json::json!(msg),
                Err(e) => return serde_json::json!({"error": e.to_string()}),
            }
        }
        "auto_get_chat_id" => {
            let bot_token = args
                .get("bot_token")
                .or(args.get("botToken"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            // 读取 API URL 配置
            let api_url_opt = load_standalone_config()
                .ok()
                .map(|c| c.telegram_config.api_base_url)
                .filter(|url| url != telegram_constants::API_BASE_URL);

            let event_tx = state.event_tx.clone();

            // 发送检测开始事件
            let _ = event_tx.send(
                serde_json::json!({
                    "type": "event",
                    "event": "chat-id-detection-started",
                    "payload": {}
                })
                .to_string(),
            );

            tokio::spawn(async move {
                let mut bot = teloxide::Bot::new(&bot_token);
                if let Some(ref url_str) = api_url_opt {
                    if let Ok(url) = reqwest::Url::parse(url_str) {
                        bot = bot.set_api_url(url);
                    }
                }

                use teloxide::prelude::*;
                let mut timeout_count = 0u32;
                const MAX_TIMEOUT: u32 = 30;

                loop {
                    match bot.get_updates().send().await {
                        Ok(updates) => {
                            for update in updates {
                                if let teloxide::types::UpdateKind::Message(message) = update.kind {
                                    let chat_id = message.chat.id.0.to_string();
                                    let chat_title =
                                        message.chat.title().unwrap_or("私聊").to_string();
                                    let username = message
                                        .from
                                        .as_ref()
                                        .and_then(|u| u.username.as_ref())
                                        .map(|s| s.as_str())
                                        .unwrap_or("未知用户");

                                    let _ = event_tx.send(
                                        serde_json::json!({
                                            "type": "event",
                                            "event": "chat-id-detected",
                                            "payload": {
                                                "chat_id": chat_id,
                                                "chat_title": chat_title,
                                                "username": username,
                                                "message_text": message.text().unwrap_or(""),
                                            }
                                        })
                                        .to_string(),
                                    );
                                    return;
                                }
                            }
                        }
                        Err(e) => {
                            log_important!(warn, "获取Telegram更新失败: {}", e);
                        }
                    }

                    timeout_count += 1;
                    if timeout_count >= MAX_TIMEOUT {
                        let _ = event_tx.send(
                            serde_json::json!({
                                "type": "event",
                                "event": "chat-id-detection-timeout",
                                "payload": {}
                            })
                            .to_string(),
                        );
                        break;
                    }
                    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                }
            });

            return serde_json::json!(null);
        }
        "start_telegram_sync" => {
            let message = args
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let predefined_options: Vec<String> = args
                .get("predefined_options")
                .or(args.get("predefinedOptions"))
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            let is_markdown = args
                .get("is_markdown")
                .or(args.get("isMarkdown"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            let config = match load_standalone_config() {
                Ok(c) => c,
                Err(_) => return serde_json::json!(null),
            };

            if !config.telegram_config.enabled
                || config.telegram_config.bot_token.trim().is_empty()
                || config.telegram_config.chat_id.trim().is_empty()
            {
                return serde_json::json!(null);
            }

            let bot_token = config.telegram_config.bot_token.clone();
            let chat_id = config.telegram_config.chat_id.clone();
            let api_url = config.telegram_config.api_base_url.clone();
            let continue_reply_enabled = config.reply_config.enable_continue_reply;

            let api_url_option = if api_url == telegram_constants::API_BASE_URL {
                None
            } else {
                Some(api_url)
            };

            let event_tx = state.event_tx.clone();

            tokio::spawn(async move {
                let core = match crate::telegram::TelegramCore::new_with_api_url(
                    bot_token,
                    chat_id,
                    api_url_option,
                ) {
                    Ok(c) => c,
                    Err(e) => {
                        log_important!(error, "创建Telegram核心失败: {}", e);
                        return;
                    }
                };

                // 发送选项消息
                if let Err(e) = core
                    .send_options_message(&message, &predefined_options, is_markdown)
                    .await
                {
                    log_important!(error, "发送选项消息失败: {}", e);
                    return;
                }

                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

                if let Err(e) = core.send_operation_message(continue_reply_enabled).await {
                    log_important!(error, "发送操作消息失败: {}", e);
                    return;
                }

                // 监听 Telegram 消息
                use teloxide::prelude::*;
                let mut offset = 0i32;
                let mut selected_options = std::collections::HashSet::<String>::new();
                let mut options_message_id: Option<i32> = None;
                let mut user_input = String::new();
                let has_options = !predefined_options.is_empty();

                if let Ok(updates) = core.bot.get_updates().limit(10).await {
                    if let Some(update) = updates.last() {
                        offset = update.id.0 as i32 + 1;
                    }
                }

                loop {
                    match core.bot.get_updates().offset(offset).timeout(10).await {
                        Ok(updates) => {
                            for update in updates {
                                offset = update.id.0 as i32 + 1;
                                match update.kind {
                                    teloxide::types::UpdateKind::CallbackQuery(callback_query) => {
                                        if has_options {
                                            if let Some(msg) = &callback_query.message {
                                                if options_message_id.is_none() {
                                                    options_message_id = Some(msg.id().0);
                                                }
                                            }
                                            if let Ok(Some(option)) =
                                                crate::telegram::handle_callback_query(
                                                    &core.bot,
                                                    &callback_query,
                                                    core.chat_id,
                                                )
                                                .await
                                            {
                                                let selected = if selected_options.contains(&option)
                                                {
                                                    selected_options.remove(&option);
                                                    false
                                                } else {
                                                    selected_options.insert(option.clone());
                                                    true
                                                };

                                                let _ = event_tx.send(serde_json::json!({
                                                    "type": "event",
                                                    "event": "telegram-event",
                                                    "payload": {"OptionToggled": {"option": option, "selected": selected}}
                                                }).to_string());

                                                if let Some(msg_id) = options_message_id {
                                                    let selected_vec: Vec<String> =
                                                        selected_options.iter().cloned().collect();
                                                    let _ = core
                                                        .update_inline_keyboard(
                                                            msg_id,
                                                            &predefined_options,
                                                            &selected_vec,
                                                        )
                                                        .await;
                                                }
                                            }
                                        }
                                    }
                                    teloxide::types::UpdateKind::Message(msg) => {
                                        if has_options {
                                            if let Some(inline_keyboard) = msg.reply_markup() {
                                                let mut contains_our_options = false;
                                                for row in &inline_keyboard.inline_keyboard {
                                                    for button in row {
                                                        if let teloxide::types::InlineKeyboardButtonKind::CallbackData(data) = &button.kind {
                                                            if data.starts_with("toggle:") {
                                                                contains_our_options = true;
                                                                break;
                                                            }
                                                        }
                                                    }
                                                    if contains_our_options {
                                                        break;
                                                    }
                                                }
                                                if contains_our_options {
                                                    options_message_id = Some(msg.id.0);
                                                }
                                            }
                                        }

                                        if let Ok(Some(event)) =
                                            crate::telegram::handle_text_message(
                                                &msg,
                                                core.chat_id,
                                                None,
                                            )
                                            .await
                                        {
                                            match &event {
                                                crate::telegram::TelegramEvent::SendPressed => {
                                                    let selected_list: Vec<String> =
                                                        selected_options.iter().cloned().collect();
                                                    let feedback = crate::telegram::core::build_feedback_message(&selected_list, &user_input, false);
                                                    let _ = core.send_message(&feedback).await;
                                                }
                                                crate::telegram::TelegramEvent::ContinuePressed => {
                                                    let feedback = crate::telegram::core::build_feedback_message(&[], "", true);
                                                    let _ = core.send_message(&feedback).await;
                                                }
                                                crate::telegram::TelegramEvent::TextUpdated {
                                                    text,
                                                } => {
                                                    user_input = text.clone();
                                                }
                                                _ => {}
                                            }

                                            let _ = event_tx.send(
                                                serde_json::json!({
                                                    "type": "event",
                                                    "event": "telegram-event",
                                                    "payload": event
                                                })
                                                .to_string(),
                                            );
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                        Err(_) => {
                            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                        }
                    }
                    tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
                }
            });

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
fn build_tauri_shim(
    request: Option<&PopupRequest>,
    _config_json: &str,
    auth_token: &str,
) -> String {
    let request_json = request
        .map(|r| serde_json::to_string(r).unwrap_or_default())
        .unwrap_or_else(|| "null".to_string());

    format!(
        r#"
/* 恒境 Web Mode - Tauri API Shim */
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
  document.title = '恒境';

  // 音频通知：监听 audio-notification 事件，使用 Web Audio API 播放通知音
  (function() {{
    var audioCtx = null;
    function getAudioCtx() {{
      if (!audioCtx) audioCtx = new (window.AudioContext || window.webkitAudioContext)();
      return audioCtx;
    }}
    // 注册事件监听（通过 Tauri shim 的 event 系统）
    if (!eventHandlers['audio-notification']) eventHandlers['audio-notification'] = [];
    eventHandlers['audio-notification'].push({{ id: ++listenerId, handler: function() {{
      try {{
        var ctx = getAudioCtx();
        // 尝试从配置加载自定义音频 URL
        window.__TAURI_INTERNALS__.invoke('get_audio_url').then(function(url) {{
          if (url && url !== '') {{
            // 使用自定义音频 URL
            var audio = new Audio(url);
            audio.play().catch(function() {{}});
          }} else {{
            // 默认：使用 Web Audio API 生成简短的提示音
            var osc = ctx.createOscillator();
            var gain = ctx.createGain();
            osc.connect(gain);
            gain.connect(ctx.destination);
            osc.frequency.value = 800;
            gain.gain.value = 0.3;
            osc.start(ctx.currentTime);
            gain.gain.exponentialRampToValueAtTime(0.001, ctx.currentTime + 0.3);
            osc.stop(ctx.currentTime + 0.3);
          }}
        }}).catch(function() {{
          // 默认提示音
          var osc = ctx.createOscillator();
          var gain = ctx.createGain();
          osc.connect(gain);
          gain.connect(ctx.destination);
          osc.frequency.value = 800;
          gain.gain.value = 0.3;
          osc.start(ctx.currentTime);
          gain.gain.exponentialRampToValueAtTime(0.001, ctx.currentTime + 0.3);
          osc.stop(ctx.currentTime + 0.3);
        }});
      }} catch(e) {{ console.warn('[恒境 Web] 音频通知失败:', e); }}
    }} }});
  }})();

  console.log('[恒境 Web] Tauri API Shim 已加载');
}})();
"#
    )
}

/// 构建配置 JSON（供 WebSocket invoke 使用）
fn build_config_json() -> String {
    let config = load_standalone_config().unwrap_or_default();
    serde_json::to_string(&config).unwrap_or_default()
}
