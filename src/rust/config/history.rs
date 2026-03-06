//! 对话历史记录管理
//!
//! 持久化存储最近的对话记录（AI 请求 + 用户响应）

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

const MAX_HISTORY_RECORDS: usize = 5;

/// 单条对话记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationRecord {
    /// 请求 ID
    pub id: String,
    /// AI 请求消息
    pub request_message: String,
    /// 预定义选项
    pub request_options: Vec<String>,
    /// 用户输入文本
    pub response_text: String,
    /// 用户选中的选项
    pub selected_options: Vec<String>,
    /// 时间戳
    pub timestamp: String,
    /// 来源 (popup / popup_continue / popup_enhance / popup_timeout_auto_submit)
    pub source: String,
}

/// 对话历史
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ConversationHistory {
    pub records: Vec<ConversationRecord>,
}

/// 获取历史文件路径
fn get_history_path() -> Result<PathBuf> {
    let config_dir = dirs::config_dir()
        .ok_or_else(|| anyhow::anyhow!("无法获取配置目录"))?
        .join("continuum");
    fs::create_dir_all(&config_dir)?;
    Ok(config_dir.join("history.json"))
}

/// 加载对话历史
pub fn load_history() -> ConversationHistory {
    match get_history_path() {
        Ok(path) => {
            if path.exists() {
                fs::read_to_string(&path)
                    .ok()
                    .and_then(|s| serde_json::from_str(&s).ok())
                    .unwrap_or_default()
            } else {
                ConversationHistory::default()
            }
        }
        Err(_) => ConversationHistory::default(),
    }
}

/// 保存对话历史
fn save_history(history: &ConversationHistory) -> Result<()> {
    let path = get_history_path()?;
    let json = serde_json::to_string_pretty(history)?;
    fs::write(&path, json)?;
    Ok(())
}

/// 追加一条记录（超过上限时移除最旧的）
pub fn append_record(record: ConversationRecord) -> Result<()> {
    let mut history = load_history();
    history.records.push(record);
    if history.records.len() > MAX_HISTORY_RECORDS {
        let drain_count = history.records.len() - MAX_HISTORY_RECORDS;
        history.records.drain(..drain_count);
    }
    save_history(&history)
}

// ─── Tauri 命令 ───

/// 保存一条对话记录
#[tauri::command]
pub async fn save_conversation_record(
    request: serde_json::Value,
    response: serde_json::Value,
) -> Result<(), String> {
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

    let record = ConversationRecord {
        id,
        request_message,
        request_options,
        response_text,
        selected_options,
        timestamp,
        source,
    };

    append_record(record).map_err(|e| format!("保存对话记录失败: {}", e))
}

/// 获取对话历史
#[tauri::command]
pub async fn get_conversation_history() -> Result<Vec<ConversationRecord>, String> {
    Ok(load_history().records)
}
