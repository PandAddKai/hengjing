//! IPC 服务器
//!
//! 在 UI 进程中运行，监听来自 MCP 的请求

use anyhow::Result;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, oneshot, Mutex};

use super::{get_socket_path, IpcRequest, IpcResponse};
use crate::log_important;

/// 等待中的请求
pub struct PendingRequest {
    pub request: IpcRequest,
    pub response_tx: oneshot::Sender<String>,
}

/// IPC 服务器状态
pub struct IpcServerState {
    /// 当前等待响应的请求
    pending_request: Mutex<Option<PendingRequest>>,
    /// 新请求通道发送端
    request_tx: mpsc::Sender<IpcRequest>,
}

impl IpcServerState {
    pub fn new(request_tx: mpsc::Sender<IpcRequest>) -> Self {
        Self {
            pending_request: Mutex::new(None),
            request_tx,
        }
    }

    /// 等待挂起槽位空闲，然后原子地设置新的挂起请求
    ///
    /// 如果已有请求正在等待用户响应，新请求会排队等待，
    /// 防止覆盖导致旧请求的 IPC 客户端收到错误并回退到启动新进程。
    pub async fn wait_and_set_pending(
        &self,
        request: IpcRequest,
    ) -> Option<oneshot::Receiver<String>> {
        let max_wait = std::time::Duration::from_secs(600);
        let start = std::time::Instant::now();

        loop {
            {
                let mut pending = self.pending_request.lock().await;
                if pending.is_none() {
                    let (response_tx, response_rx) = oneshot::channel();
                    *pending = Some(PendingRequest { request, response_tx });
                    return Some(response_rx);
                }
            }

            if start.elapsed() > max_wait {
                log_important!(warn, "等待挂起槽位超时（600s），放弃请求: {}", request.id);
                return None;
            }

            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        }
    }

    /// 发送响应给等待中的请求
    pub async fn send_response(&self, request_id: &str, response: String) -> Result<()> {
        let mut pending = self.pending_request.lock().await;
        if let Some(req) = pending.take() {
            if req.request.id == request_id {
                let _ = req.response_tx.send(response);
                Ok(())
            } else {
                // 请求 ID 不匹配，放回去
                *pending = Some(req);
                anyhow::bail!("请求 ID 不匹配")
            }
        } else {
            anyhow::bail!("没有等待中的请求")
        }
    }

    /// 取消指定请求的挂起状态（客户端断开时调用）
    pub async fn cancel_pending(&self, request_id: &str) {
        let mut pending = self.pending_request.lock().await;
        let should_remove = pending
            .as_ref()
            .map(|req| req.request.id == request_id)
            .unwrap_or(false);
        if should_remove {
            // take() 会 drop PendingRequest，其中的 response_tx 被丢弃
            // 这样 response_rx 端会收到 Err，但此时已无人在等待
            let _ = pending.take();
            log_important!(info, "已取消挂起请求（客户端断开）: {}", request_id);
        }
    }

    /// 获取请求发送通道
    pub fn get_request_tx(&self) -> mpsc::Sender<IpcRequest> {
        self.request_tx.clone()
    }
}

/// IPC 服务器
pub struct IpcServer {
    state: Arc<IpcServerState>,
}

impl IpcServer {
    /// 创建新的 IPC 服务器
    pub fn new(request_tx: mpsc::Sender<IpcRequest>) -> Self {
        Self {
            state: Arc::new(IpcServerState::new(request_tx)),
        }
    }

    /// 获取状态引用
    pub fn state(&self) -> Arc<IpcServerState> {
        Arc::clone(&self.state)
    }

    /// 启动 IPC 服务器
    #[cfg(unix)]
    pub async fn start(&self) -> Result<()> {
        use tokio::net::UnixListener;

        let socket_path = get_socket_path();

        // 如果 socket 文件已存在，先删除
        if socket_path.exists() {
            std::fs::remove_file(&socket_path)?;
        }

        // 创建 Unix socket 监听器
        let listener = UnixListener::bind(&socket_path)?;
        log_important!(info, "IPC 服务器已启动: {:?}", socket_path);

        let state = Arc::clone(&self.state);

        // 在后台任务中处理连接
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, _)) => {
                        let state_clone = Arc::clone(&state);
                        tokio::spawn(async move {
                            if let Err(e) = handle_connection(stream, state_clone).await {
                                log_important!(error, "处理 IPC 连接失败: {}", e);
                            }
                        });
                    }
                    Err(e) => {
                        log_important!(error, "接受 IPC 连接失败: {}", e);
                    }
                }
            }
        });

        Ok(())
    }

    #[cfg(windows)]
    pub async fn start(&self) -> Result<()> {
        log_important!(warn, "Windows IPC 服务器暂未实现");
        Ok(())
    }
}

/// 处理单个 IPC 连接
#[cfg(unix)]
async fn handle_connection(
    stream: tokio::net::UnixStream,
    state: Arc<IpcServerState>,
) -> Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut buf_reader = BufReader::new(reader);
    let mut line = String::new();

    // 读取请求
    let bytes_read = buf_reader.read_line(&mut line).await?;
    if bytes_read == 0 {
        return Ok(());
    }

    // 解析请求
    let request: IpcRequest = serde_json::from_str(line.trim())?;
    let request_id = request.id.clone();

    log_important!(info, "收到 IPC 请求: {}，等待挂起槽位...", request_id);

    // 等待挂起槽位空闲，然后原子地设置（不会覆盖正在处理的请求）
    let response_rx = match state.wait_and_set_pending(request.clone()).await {
        Some(rx) => rx,
        None => {
            // 等待超时
            let ipc_response = IpcResponse {
                id: request_id,
                response: String::new(),
                success: false,
                error: Some("等待处理超时，当前有其他请求正在等待用户响应".to_string()),
            };
            let response_json = serde_json::to_string(&ipc_response)?;
            writer.write_all(response_json.as_bytes()).await?;
            writer.write_all(b"\n").await?;
            writer.flush().await?;
            return Ok(());
        }
    };

    log_important!(info, "请求 {} 已获得挂起槽位，通知前端", request_id);

    // 通知前端有新请求
    let _ = state.get_request_tx().send(request).await;

    // 同时监听：用户响应 OR 客户端断开（MCP 进程被 Cursor 重启）
    // 如果客户端断开而不清理 pending，新请求会被 wait_and_set_pending 卡住，
    // 最终导致 IPC 超时 → 回退启动新进程 → 出现第二个窗口。
    let mut disconnect_buf = [0u8; 1];
    tokio::select! {
        result = response_rx => {
            // 正常路径：用户通过前端提交了响应
            match result {
                Ok(response) => {
                    let ipc_response = IpcResponse {
                        id: request_id,
                        response,
                        success: true,
                        error: None,
                    };
                    let response_json = serde_json::to_string(&ipc_response)?;
                    writer.write_all(response_json.as_bytes()).await?;
                    writer.write_all(b"\n").await?;
                    writer.flush().await?;
                }
                Err(_) => {
                    let ipc_response = IpcResponse {
                        id: request_id,
                        response: String::new(),
                        success: false,
                        error: Some("响应通道已关闭".to_string()),
                    };
                    let response_json = serde_json::to_string(&ipc_response)?;
                    writer.write_all(response_json.as_bytes()).await?;
                    writer.write_all(b"\n").await?;
                    writer.flush().await?;
                }
            }
        }
        result = buf_reader.read(&mut disconnect_buf) => {
            // 客户端断开：MCP 进程被 Cursor 杀掉或重启
            // 清理 pending 槽位，让新请求能立即获得槽位
            match result {
                Ok(0) | Err(_) => {
                    log_important!(warn, "IPC 客户端已断开（请求 {}），释放挂起槽位", request_id);
                    state.cancel_pending(&request_id).await;
                }
                Ok(_) => {
                    // 客户端发送了额外数据（不应该发生），忽略
                }
            }
        }
    }

    Ok(())
}

/// 清理 socket 文件
pub fn cleanup_socket() {
    let socket_path = get_socket_path();
    if socket_path.exists() {
        let _ = std::fs::remove_file(&socket_path);
    }
}
