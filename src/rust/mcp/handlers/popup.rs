use anyhow::Result;
use std::process::Command;
use std::fs;
use std::path::Path;

use crate::mcp::types::PopupRequest;
use crate::ipc::{IpcRequest, client::IpcClient};
use crate::log_important;

/// 创建 Tauri 弹窗
///
/// 优先通过 IPC 发送到已运行的 UI，失败则启动新进程
pub async fn create_tauri_popup(request: &PopupRequest) -> Result<String> {
    // 尝试通过 IPC 发送到已运行的 UI
    let ipc_request = IpcRequest::from(request);

    // 首先检查 UI 是否在运行
    let ui_running = IpcClient::is_ui_running().await;

    if ui_running {
        log_important!(info, "检测到 UI 正在运行，通过 IPC 发送请求");
        match IpcClient::send_request(&ipc_request).await {
            Ok(response) => {
                log_important!(info, "IPC 响应成功");
                return Ok(response);
            }
            Err(e) => {
                log_important!(warn, "IPC 请求失败: {}，回退到启动新进程", e);
            }
        }
    }

    // IPC 失败或 UI 未运行，启动新进程（同步阻塞操作，放到 spawn_blocking 中）
    log_important!(info, "启动新的 UI 进程");
    let request_clone = request.clone();
    tokio::task::spawn_blocking(move || create_new_ui_process(&request_clone))
        .await?
}

/// 启动新的 UI 进程处理请求
fn create_new_ui_process(request: &PopupRequest) -> Result<String> {
    // 创建临时请求文件 - 跨平台适配
    let temp_dir = std::env::temp_dir();
    let temp_file = temp_dir.join(format!("mcp_request_{}.json", request.id));
    let request_json = serde_json::to_string_pretty(request)?;
    fs::write(&temp_file, request_json)?;

    // 尝试找到等命令的路径
    let command_path = find_ui_command()?;

    // 调用等命令
    let output = Command::new(&command_path)
        .arg("--mcp-request")
        .arg(temp_file.to_string_lossy().to_string())
        .output()?;

    // 清理临时文件
    let _ = fs::remove_file(&temp_file);

    if output.status.success() {
        let response = String::from_utf8_lossy(&output.stdout);
        let response = response.trim();
        if response.is_empty() {
            Ok("用户取消了操作".to_string())
        } else {
            Ok(response.to_string())
        }
    } else {
        let error = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("UI进程失败: {}", error);
    }
}

/// 查找等 UI 命令的路径
fn find_ui_command() -> Result<String> {
    // 1. 优先尝试与当前 MCP 服务器同目录的等命令
    if let Ok(current_exe) = std::env::current_exe() {
        if let Some(exe_dir) = current_exe.parent() {
            let local_ui_path = exe_dir.join("等");
            if local_ui_path.exists() && is_executable(&local_ui_path) {
                return Ok(local_ui_path.to_string_lossy().to_string());
            }
        }
    }

    // 2. 尝试全局命令
    if test_command_available("等") {
        return Ok("等".to_string());
    }

    anyhow::bail!(
        "找不到等 UI 命令。请确保：\n\
         1. 已编译项目：cargo build --release\n\
         2. 或已全局安装：./install.sh\n\
         3. 或等命令在同目录下"
    )
}

fn test_command_available(command: &str) -> bool {
    Command::new(command)
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.metadata()
            .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }

    #[cfg(windows)]
    {
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.eq_ignore_ascii_case("exe"))
            .unwrap_or(false)
    }
}
