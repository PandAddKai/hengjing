//! IPC Tauri 命令

use std::sync::Arc;
use tauri::{AppHandle, State};
use tokio::sync::Mutex;

use super::server::IpcServerState;
use super::IpcRequest;
use crate::log_important;

/// IPC 服务器状态包装器
pub struct IpcStateWrapper(pub Arc<Mutex<Option<Arc<IpcServerState>>>>);

impl Default for IpcStateWrapper {
    fn default() -> Self {
        Self(Arc::new(Mutex::new(None)))
    }
}

/// 发送 IPC 响应
#[tauri::command]
pub async fn send_ipc_response(
    request_id: String,
    response: String,
    ipc_state: State<'_, IpcStateWrapper>,
) -> Result<(), String> {
    let state_guard = ipc_state.0.lock().await;
    if let Some(state) = state_guard.as_ref() {
        state
            .send_response(&request_id, response)
            .await
            .map_err(|e| format!("发送 IPC 响应失败: {}", e))
    } else {
        Err("IPC 服务器未初始化".to_string())
    }
}

/// 启动 IPC 服务器并监听请求
pub async fn start_ipc_server(
    app_handle: &AppHandle,
    ipc_state: Arc<Mutex<Option<Arc<IpcServerState>>>>,
) -> Result<(), String> {
    use super::server::IpcServer;
    use tokio::sync::mpsc;

    // 创建请求通道
    let (request_tx, mut request_rx) = mpsc::channel::<IpcRequest>(32);

    // 创建并启动 IPC 服务器
    let server = IpcServer::new(request_tx);
    let server_state = server.state();
    let request_state = server_state.clone();

    // 保存服务器状态
    {
        let mut state_guard = ipc_state.lock().await;
        *state_guard = Some(server_state);
    }

    server
        .start()
        .await
        .map_err(|e| format!("启动 IPC 服务器失败: {}", e))?;

    // 在后台任务中监听请求并为每个请求创建独立 popup 窗口
    let app_handle_clone = app_handle.clone();
    tokio::spawn(async move {
        while let Some(request) = request_rx.recv().await {
            let request_id = request.id.clone();
            log_important!(
                info,
                "收到 IPC 请求，准备创建独立 popup 窗口: {}",
                request_id
            );

            let popup_request: crate::mcp::types::PopupRequest = request.into();
            if let Err(e) =
                crate::ui::popup_windows::open_popup_window(&app_handle_clone, popup_request).await
            {
                log_important!(error, "创建 popup 窗口失败 {}: {}", request_id, e);
                request_state.cancel_pending(&request_id).await;
            }
        }
    });

    Ok(())
}
