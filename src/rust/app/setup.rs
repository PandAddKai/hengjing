use anyhow::Result;
use tauri::{AppHandle, Manager};

use crate::config::{AppState, load_config_and_apply_window_settings};
use crate::ipc::{IpcStateWrapper, start_ipc_server};

/// 应用启动时的初始化逻辑
pub async fn setup_application(app: &AppHandle) -> Result<()> {
    // 1) 加载配置并应用窗口设置
    let state = app.state::<AppState>();
    load_config_and_apply_window_settings(&state, app).await?;

    // 2) 启动 IPC 服务器（用于向已运行 UI 发送 MCP 请求）
    let ipc_state = app.state::<IpcStateWrapper>();
    start_ipc_server(app, ipc_state.0.clone())
        .await
        .map_err(|e| anyhow::anyhow!(e))?;

    Ok(())
}
