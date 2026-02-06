//! IPC 服务器
//!
//! 在 UI 进程中运行，监听来自 MCP 的请求

use anyhow::Result;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
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
    
    /// 设置当前等待的请求
    pub async fn set_pending(&self, request: IpcRequest, response_tx: oneshot::Sender<String>) {
        let mut pending = self.pending_request.lock().await;
        *pending = Some(PendingRequest { request, response_tx });
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
    
    log_important!(info, "收到 IPC 请求: {}", request_id);
    
    // 创建响应通道
    let (response_tx, response_rx) = oneshot::channel();
    
    // 设置等待中的请求
    state.set_pending(request.clone(), response_tx).await;
    
    // 通知前端有新请求
    let _ = state.get_request_tx().send(request).await;
    
    // 等待响应
    match response_rx.await {
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
    
    Ok(())
}

/// 清理 socket 文件
pub fn cleanup_socket() {
    let socket_path = get_socket_path();
    if socket_path.exists() {
        let _ = std::fs::remove_file(&socket_path);
    }
}
