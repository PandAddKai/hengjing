use crate::config::settings::AppConfig;
use crate::config::{get_standalone_config_path, load_standalone_config, save_standalone_config};
use crate::constants::font;
use crate::log_important;
use serde_json::Value;

/// Web 模式下统一的 invoke 命令分发器
///
/// 处理所有配置读写命令，GUI 专属命令返回合理默认值。
/// 新增 Tauri 命令时，只需在此处对应添加即可。
pub fn dispatch(cmd: &str, args: &Value) -> Option<Value> {
    match cmd {
        // === 配置读取 ===
        "get_reply_config" => Some(read_config(|c| {
            serde_json::json!({
                "enable_continue_reply": c.reply_config.enable_continue_reply,
                "auto_continue_threshold": c.reply_config.auto_continue_threshold,
                "continue_prompt": c.reply_config.continue_prompt,
            })
        })),
        "get_timeout_auto_submit_config" => Some(read_config(|c| {
            serde_json::json!({
                "enabled": c.timeout_auto_submit_config.enabled,
                "timeout_seconds": c.timeout_auto_submit_config.timeout_seconds,
                "prompt_source": c.timeout_auto_submit_config.prompt_source,
                "custom_prompt_id": c.timeout_auto_submit_config.custom_prompt_id,
                "manual_prompt": c.timeout_auto_submit_config.manual_prompt,
            })
        })),
        "get_custom_prompt_config" => Some(read_config(|c| {
            serde_json::to_value(&c.custom_prompt_config).unwrap_or(serde_json::json!({}))
        })),
        "get_telegram_config" => Some(read_config(|c| {
            serde_json::to_value(&c.telegram_config).unwrap_or(serde_json::json!({}))
        })),
        "get_app_config" => Some(read_config(|c| {
            serde_json::to_value(&c).unwrap_or(serde_json::json!({}))
        })),
        "get_ui_config" => Some(read_config(|c| {
            serde_json::to_value(&c.ui_config).unwrap_or(serde_json::json!({}))
        })),
        "get_audio_config" => Some(read_config(|c| {
            serde_json::to_value(&c.audio_config).unwrap_or(serde_json::json!({}))
        })),
        "get_shortcut_config" => Some(read_config(|c| {
            serde_json::to_value(&c.shortcut_config).unwrap_or(serde_json::json!({}))
        })),
        "get_theme" => Some(read_config(|c| serde_json::json!(c.ui_config.theme))),
        "get_font_config" => Some(read_config(|c| {
            serde_json::to_value(&c.ui_config.font_config).unwrap_or(serde_json::json!({}))
        })),
        "get_mcp_tools_config" => Some(read_config(|c| {
            serde_json::to_value(&c.mcp_config.tools).unwrap_or(serde_json::json!({}))
        })),
        "get_app_info" => Some(serde_json::json!({
            "name": "恒境",
            "version": env!("CARGO_PKG_VERSION"),
            "mode": "web"
        })),
        "get_config_file_path" => Some(match get_standalone_config_path() {
            Ok(path) => serde_json::json!(path.to_string_lossy()),
            Err(_) => Value::Null,
        }),

        // === 配置写入 ===
        "set_reply_config" => Some(write_config(args, |config, args| {
            if let Some(val) = extract_nested(args, &["replyConfig", "reply_config"]) {
                if let Ok(v) = serde_json::from_value(val) {
                    config.reply_config = v;
                    return;
                }
            }
            if let Ok(v) = serde_json::from_value(args.clone()) {
                config.reply_config = v;
            }
        })),
        "set_timeout_auto_submit_config" => Some(write_config(args, |config, args| {
            if let Some(val) = extract_nested(
                args,
                &["timeoutAutoSubmitConfig", "timeout_auto_submit_config"],
            ) {
                if let Ok(v) = serde_json::from_value(val) {
                    config.timeout_auto_submit_config = v;
                    return;
                }
            }
            if let Ok(v) = serde_json::from_value(args.clone()) {
                config.timeout_auto_submit_config = v;
            }
        })),
        "set_telegram_config" => Some(write_config(args, |config, args| {
            if let Some(val) = extract_nested(args, &["telegramConfig", "telegram_config"]) {
                if let Ok(v) = serde_json::from_value(val) {
                    config.telegram_config = v;
                    return;
                }
            }
            if let Ok(v) = serde_json::from_value(args.clone()) {
                config.telegram_config = v;
            }
        })),
        "set_theme" => Some(write_config(args, |config, args| {
            if let Some(theme) = args.get("theme").and_then(|t| t.as_str()) {
                config.ui_config.theme = theme.to_string();
            }
        })),
        "set_audio_notification_enabled" => Some(write_config(args, |config, args| {
            if let Some(enabled) = args.get("enabled").and_then(|e| e.as_bool()) {
                config.audio_config.notification_enabled = enabled;
            }
        })),
        "set_audio_url" => Some(write_config(args, |config, args| {
            if let Some(url) = args.get("url").and_then(|u| u.as_str()) {
                config.audio_config.custom_url = url.to_string();
            }
        })),
        "set_custom_prompt_enabled" => Some(write_config(args, |config, args| {
            if let Some(enabled) = args.get("enabled").and_then(|e| e.as_bool()) {
                config.custom_prompt_config.enabled = enabled;
            }
        })),
        "add_custom_prompt" => Some(write_config(args, |config, args| {
            if let Some(prompt_val) = args.get("prompt") {
                if let Ok(prompt) = serde_json::from_value::<crate::config::settings::CustomPrompt>(
                    prompt_val.clone(),
                ) {
                    config.custom_prompt_config.prompts.push(prompt);
                }
            }
        })),
        "update_custom_prompt" => Some(write_config(args, |config, args| {
            if let Some(prompt_val) = args.get("prompt") {
                if let Ok(prompt) = serde_json::from_value::<crate::config::settings::CustomPrompt>(
                    prompt_val.clone(),
                ) {
                    if let Some(existing) = config
                        .custom_prompt_config
                        .prompts
                        .iter_mut()
                        .find(|p| p.id == prompt.id)
                    {
                        *existing = prompt;
                    }
                }
            }
        })),
        "delete_custom_prompt" => Some(write_config(args, |config, args| {
            if let Some(prompt_id) = args
                .get("promptId")
                .or(args.get("prompt_id"))
                .and_then(|id| id.as_str())
            {
                config
                    .custom_prompt_config
                    .prompts
                    .retain(|p| p.id != prompt_id);
            }
        })),
        "update_conditional_prompt_state" => Some(write_config(args, |config, args| {
            if let Some(prompt_id) = args
                .get("promptId")
                .or(args.get("prompt_id"))
                .and_then(|id| id.as_str())
            {
                if let Some(state) = args
                    .get("state")
                    .or(args.get("currentState"))
                    .and_then(|s| s.as_bool())
                {
                    if let Some(prompt) = config
                        .custom_prompt_config
                        .prompts
                        .iter_mut()
                        .find(|p| p.id == prompt_id)
                    {
                        prompt.current_state = state;
                    }
                }
            }
        })),
        "update_custom_prompt_order" => Some(write_config(args, |config, args| {
            if let Some(prompt_id) = args
                .get("promptId")
                .or(args.get("prompt_id"))
                .and_then(|id| id.as_str())
            {
                if let Some(state) = args
                    .get("state")
                    .or(args.get("currentState"))
                    .and_then(|s| s.as_bool())
                {
                    if let Some(prompt) = config
                        .custom_prompt_config
                        .prompts
                        .iter_mut()
                        .find(|p| p.id == prompt_id)
                    {
                        prompt.current_state = state;
                    }
                }
            }
        })),
        "set_mcp_tool_enabled" => Some(write_config(args, |config, args| {
            if let Some(obj) = args.as_object() {
                if let (Some(tool_name), Some(enabled)) = (
                    obj.get("toolName")
                        .or(obj.get("tool_name"))
                        .and_then(|t| t.as_str()),
                    obj.get("enabled").and_then(|e| e.as_bool()),
                ) {
                    config
                        .mcp_config
                        .tools
                        .insert(tool_name.to_string(), enabled);
                }
            }
        })),
        "set_font_family" => Some(write_config(args, |config, args| {
            if let Some(font) = args
                .get("fontFamily")
                .or(args.get("font_family"))
                .and_then(|f| f.as_str())
            {
                config.ui_config.font_config.font_family = font.to_string();
            }
        })),
        "set_font_size" => Some(write_config(args, |config, args| {
            if let Some(size) = args
                .get("fontSize")
                .or(args.get("font_size"))
                .and_then(|s| s.as_str())
            {
                config.ui_config.font_config.font_size = size.to_string();
            }
        })),
        "set_custom_font_family" => Some(write_config(args, |config, args| {
            if let Some(font) = args
                .get("customFontFamily")
                .or(args.get("custom_font_family"))
                .and_then(|f| f.as_str())
            {
                config.ui_config.font_config.custom_font_family = font.to_string();
            }
        })),
        "update_shortcut_binding" => Some(write_config(args, |config, args| {
            if let (Some(action), Some(key_val)) = (
                args.get("action").and_then(|a| a.as_str()),
                args.get("keyCombination").or(args.get("key_combination")),
            ) {
                if let Some(shortcut) = config.shortcut_config.shortcuts.get_mut(action) {
                    if let Ok(key) = serde_json::from_value(key_val.clone()) {
                        shortcut.key_combination = key;
                    }
                }
            }
        })),
        "reset_shortcuts_to_default" => Some(write_config(args, |config, _| {
            config.shortcut_config = crate::config::settings::default_shortcut_config();
        })),

        // === GUI 专属（Web 模式返回安全默认值）===
        "get_font_family_options" => Some(serde_json::json!(font::FONT_FAMILIES
            .iter()
            .map(|(id, name, css_value)| {
                serde_json::json!({
                    "id": id,
                    "name": name,
                    "css_value": css_value,
                })
            })
            .collect::<Vec<_>>())),
        "get_font_size_options" => Some(serde_json::json!(font::FONT_SIZES
            .iter()
            .map(|(id, name, scale)| {
                serde_json::json!({
                    "id": id,
                    "name": name,
                    "scale": scale,
                })
            })
            .collect::<Vec<_>>())),
        "get_available_audio_assets" => Some(serde_json::json!([])),
        "get_mcp_tools_status" => Some(read_config(|c| {
            serde_json::to_value(&c.mcp_config.tools).unwrap_or(serde_json::json!({}))
        })),

        "get_always_on_top" => Some(read_config(|c| {
            serde_json::json!(c.ui_config.always_on_top)
        })),
        "get_audio_notification_enabled" => Some(read_config(|c| {
            serde_json::json!(c.audio_config.notification_enabled)
        })),
        "get_audio_url" => Some(read_config(|c| {
            serde_json::json!(c.audio_config.custom_url)
        })),
        "get_layout_mode" => Some(read_config(|c| serde_json::json!(c.ui_config.layout_mode))),
        "set_layout_mode" => Some(write_config(args, |config, args| {
            if let Some(mode) = args
                .get("layoutMode")
                .or(args.get("layout_mode"))
                .and_then(|m| m.as_str())
            {
                config.ui_config.layout_mode = mode.to_string();
            }
        })),
        "get_current_version" => Some(serde_json::json!(env!("CARGO_PKG_VERSION"))),
        "get_window_config" => Some(read_config(|c| {
            serde_json::to_value(&c.ui_config.window_config).unwrap_or(serde_json::json!({}))
        })),
        "reload_config" => Some(Value::Null),
        "reset_font_config" => Some(write_config(args, |config, _| {
            config.ui_config.font_config = crate::config::settings::default_font_config();
        })),
        "reset_mcp_tools_config" => Some(write_config(args, |config, _| {
            config.mcp_config.tools = crate::config::settings::default_mcp_tools();
        })),
        "open_external_url" => {
            if let Some(url) = args.get("url").and_then(|u| u.as_str()) {
                let _ = open::that(url);
            }
            Some(Value::Null)
        }

        // 对话历史
        "save_conversation_record" => {
            let record = build_conversation_record(args);
            if let Some(record) = record {
                if let Err(e) = crate::config::history::append_record(record) {
                    log_important!(error, "保存对话记录失败: {}", e);
                }
            }
            Some(Value::Null)
        }
        "get_conversation_history" => {
            let history = crate::config::history::load_history();
            Some(serde_json::to_value(&history.records).unwrap_or(serde_json::json!([])))
        }

        // === 真正的 GUI 专属（Web 模式无法实现）===
        "get_window_constraints_cmd"
        | "get_current_window_size"
        | "get_window_settings_for_mode"
        | "get_window_settings"
        | "sync_window_state"
        | "set_always_on_top"
        | "set_window_settings"
        | "set_window_config"
        | "apply_window_constraints"
        | "update_window_size"
        | "select_image_files"
        | "refresh_audio_assets"
        | "test_audio_sound"
        | "stop_audio_sound"
        | "check_for_updates"
        | "download_and_install_update"
        | "restart_app"
        | "force_exit_app"
        | "reset_exit_attempts_cmd"
        | "handle_app_exit_request"
        | "install_cli"
        | "get_cli_install_status"
        | "save_acemcp_config"
        | "test_acemcp_connection"
        | "read_acemcp_logs"
        | "clear_acemcp_cache"
        | "get_acemcp_config"
        | "debug_acemcp_search"
        | "execute_acemcp_tool"
        | "create_test_popup"
        | "build_mcp_send_response"
        | "build_mcp_continue_response" => Some(Value::Null),

        _ => None,
    }
}

fn read_config<F>(reader: F) -> Value
where
    F: FnOnce(&AppConfig) -> Value,
{
    match load_standalone_config() {
        Ok(config) => reader(&config),
        Err(e) => {
            log_important!(error, "Web 配置读取失败: {}", e);
            Value::Null
        }
    }
}

fn write_config<F>(args: &Value, modifier: F) -> Value
where
    F: FnOnce(&mut AppConfig, &Value),
{
    match load_standalone_config() {
        Ok(mut config) => {
            modifier(&mut config, args);
            if let Err(e) = save_standalone_config(&config) {
                log_important!(error, "Web 配置保存失败: {}", e);
            }
        }
        Err(e) => {
            log_important!(error, "Web 配置加载失败: {}", e);
        }
    }
    Value::Null
}

fn extract_nested(args: &Value, keys: &[&str]) -> Option<Value> {
    args.as_object()
        .and_then(|obj| keys.iter().find_map(|key| obj.get(*key).cloned()))
}

fn build_conversation_record(args: &Value) -> Option<crate::config::history::ConversationRecord> {
    let request = args.get("request")?;
    let response = args.get("response")?;

    let id = request
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let request_message = request
        .get("message")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let request_options: Vec<String> = request
        .get("predefined_options")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let response_text = response
        .get("user_input")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let selected_options: Vec<String> = response
        .get("selected_options")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let source = response
        .get("metadata")
        .and_then(|m| m.get("source"))
        .and_then(|v| v.as_str())
        .unwrap_or("popup")
        .to_string();
    let timestamp = response
        .get("metadata")
        .and_then(|m| m.get("timestamp"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    Some(crate::config::history::ConversationRecord {
        id,
        request_message,
        request_options,
        response_text,
        selected_options,
        timestamp,
        source,
    })
}
