use std::collections::HashMap;

use tauri::{AppHandle, Manager, State, WebviewUrl, WebviewWindow, WindowEvent};
use tokio::sync::Mutex;

use crate::config::AppState;
use crate::ipc::IpcStateWrapper;
use crate::log_important;
use crate::mcp::types::PopupRequest;

#[derive(Clone)]
struct PopupSession {
    request: PopupRequest,
    window_label: String,
}

#[derive(Default)]
pub struct PopupSessionState {
    sessions: Mutex<HashMap<String, PopupSession>>,
}

impl PopupSessionState {
    pub async fn register_request(&self, request: PopupRequest) -> String {
        let window_label = popup_window_label(&request.id);
        let session = PopupSession {
            request: request.clone(),
            window_label: window_label.clone(),
        };

        self.sessions.lock().await.insert(request.id, session);
        window_label
    }

    pub async fn get_request_by_window_label(&self, window_label: &str) -> Option<PopupRequest> {
        self.sessions
            .lock()
            .await
            .values()
            .find(|session| session.window_label == window_label)
            .map(|session| session.request.clone())
    }

    pub async fn is_owned_by_window(&self, request_id: &str, window_label: &str) -> bool {
        self.sessions
            .lock()
            .await
            .get(request_id)
            .map(|session| session.window_label == window_label)
            .unwrap_or(false)
    }

    pub async fn remove_request(&self, request_id: &str) -> Option<PopupRequest> {
        self.sessions
            .lock()
            .await
            .remove(request_id)
            .map(|session| session.request)
    }
}

fn popup_window_label(request_id: &str) -> String {
    format!("popup-{}", request_id)
}

fn cancel_response_json() -> String {
    serde_json::to_string("CANCELLED").unwrap_or_else(|_| "\"CANCELLED\"".to_string())
}

fn attach_popup_close_listener(window: &WebviewWindow, request_id: String) {
    let app_handle = window.app_handle().clone();
    let popup_window = window.clone();

    window.on_window_event(move |event| {
        if let WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();

            let app_handle = app_handle.clone();
            let popup_window = popup_window.clone();
            let request_id = request_id.clone();

            tauri::async_runtime::spawn(async move {
                let popup_state = app_handle.state::<PopupSessionState>();
                let was_pending = popup_state.remove_request(&request_id).await.is_some();

                if was_pending {
                    let ipc_state = app_handle.state::<IpcStateWrapper>();
                    let state_guard = ipc_state.0.lock().await;

                    if let Some(state) = state_guard.as_ref() {
                        if let Err(e) = state
                            .send_response(&request_id, cancel_response_json())
                            .await
                        {
                            log_important!(warn, "popup 关闭时取消请求失败 {}: {}", request_id, e);
                        }
                    }
                }

                if let Err(e) = popup_window.destroy() {
                    log_important!(warn, "销毁 popup 窗口失败 {}: {}", request_id, e);
                }
            });
        }
    });
}

pub async fn open_popup_window(app: &AppHandle, request: PopupRequest) -> Result<String, String> {
    let popup_state = app.state::<PopupSessionState>();
    let window_label = popup_state.register_request(request.clone()).await;

    if let Some(existing_window) = app.get_webview_window(&window_label) {
        let _ = existing_window.show();
        let _ = existing_window.set_focus();
        return Ok(window_label);
    }

    let (always_on_top, window_config) = {
        let app_state = app.state::<AppState>();
        let config = app_state
            .config
            .lock()
            .map_err(|e| format!("获取配置失败: {}", e))?;
        (
            config.ui_config.always_on_top,
            config.ui_config.window_config.clone(),
        )
    };

    let (width, height) = if window_config.fixed {
        (window_config.fixed_width, window_config.fixed_height)
    } else {
        (window_config.free_width, window_config.free_height)
    };

    let popup_window = match tauri::WebviewWindowBuilder::new(
        app,
        window_label.clone(),
        WebviewUrl::App("index.html".into()),
    )
    .title("恒境")
    .inner_size(width, height)
    .min_inner_size(window_config.min_width, window_config.min_height)
    .max_inner_size(window_config.max_width, window_config.max_height)
    .center()
    .visible(true)
    .resizable(true)
    .decorations(true)
    .always_on_top(always_on_top)
    .build()
    {
        Ok(window) => window,
        Err(e) => {
            popup_state.remove_request(&request.id).await;
            return Err(format!("创建 popup 窗口失败: {}", e));
        }
    };

    attach_popup_close_listener(&popup_window, request.id.clone());
    let _ = popup_window.set_focus();

    Ok(window_label)
}

#[tauri::command]
pub async fn get_popup_request_for_current_window(
    window: WebviewWindow,
    popup_state: State<'_, PopupSessionState>,
) -> Result<Option<PopupRequest>, String> {
    Ok(popup_state
        .get_request_by_window_label(window.label())
        .await)
}

#[tauri::command]
pub async fn send_popup_response(
    request_id: String,
    response: serde_json::Value,
    window: WebviewWindow,
    popup_state: State<'_, PopupSessionState>,
    ipc_state: State<'_, IpcStateWrapper>,
) -> Result<(), String> {
    let window_label = window.label().to_string();
    if !popup_state
        .is_owned_by_window(&request_id, &window_label)
        .await
    {
        return Err(format!(
            "popup 窗口 {} 不拥有请求 {}",
            window_label, request_id
        ));
    }

    let response_str =
        serde_json::to_string(&response).map_err(|e| format!("序列化响应失败: {}", e))?;

    {
        let state_guard = ipc_state.0.lock().await;
        let state = state_guard
            .as_ref()
            .ok_or_else(|| "IPC 服务器未初始化".to_string())?;
        state
            .send_response(&request_id, response_str)
            .await
            .map_err(|e| format!("发送 popup 响应失败: {}", e))?;
    }

    popup_state.remove_request(&request_id).await;
    window
        .destroy()
        .map_err(|e| format!("关闭 popup 窗口失败: {}", e))?;

    Ok(())
}

#[tauri::command]
pub async fn cancel_popup_request(
    request_id: String,
    window: WebviewWindow,
    popup_state: State<'_, PopupSessionState>,
    ipc_state: State<'_, IpcStateWrapper>,
) -> Result<(), String> {
    let window_label = window.label().to_string();
    if !popup_state
        .is_owned_by_window(&request_id, &window_label)
        .await
    {
        return Err(format!(
            "popup 窗口 {} 不拥有请求 {}",
            window_label, request_id
        ));
    }

    {
        let state_guard = ipc_state.0.lock().await;
        let state = state_guard
            .as_ref()
            .ok_or_else(|| "IPC 服务器未初始化".to_string())?;
        state
            .send_response(&request_id, cancel_response_json())
            .await
            .map_err(|e| format!("取消 popup 请求失败: {}", e))?;
    }

    popup_state.remove_request(&request_id).await;
    window
        .destroy()
        .map_err(|e| format!("关闭 popup 窗口失败: {}", e))?;

    Ok(())
}
