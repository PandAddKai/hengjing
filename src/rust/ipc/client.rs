//! IPC 客户端
//!
//! 用于向已运行的 UI 进程发送请求

use anyhow::{Result, Context};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::time::{timeout, Duration};

use super::{get_socket_path, IpcRequest, IpcResponse};

/// IPC 客户端
pub struct IpcClient;

impl IpcClient {
    /// 检查 UI 是否已经在运行（通过尝试连接 socket）
    pub async fn is_ui_running() -> bool {
        let socket_path = get_socket_path();
        
        #[cfg(unix)]
        {
            if !socket_path.exists() {
                return false;
            }
            
            match tokio::net::UnixStream::connect(&socket_path).await {
                Ok(_) => true,
                Err(_) => false,
            }
        }
        
        #[cfg(windows)]
        {
            false // Windows 暂时返回 false，需要额外实现
        }
    }
    
    /// 发送请求到已运行的 UI 进程
    ///
    /// 返回响应字符串，或者在连接失败时返回错误
    pub async fn send_request(request: &IpcRequest) -> Result<String> {
        let socket_path = get_socket_path();
        
        #[cfg(unix)]
        {
            Self::send_request_unix(&socket_path, request).await
        }
        
        #[cfg(windows)]
        {
            anyhow::bail!("Windows IPC 暂未实现")
        }
    }
    
    #[cfg(unix)]
    async fn send_request_unix(socket_path: &std::path::Path, request: &IpcRequest) -> Result<String> {
        use tokio::net::UnixStream;
        
        // 连接到 socket
        let stream = UnixStream::connect(socket_path)
            .await
            .context("无法连接到 UI 进程")?;
        
        let (reader, mut writer) = stream.into_split();
        
        // 发送请求
        let request_json = serde_json::to_string(request)?;
        writer.write_all(request_json.as_bytes()).await?;
        writer.write_all(b"\n").await?;
        writer.flush().await?;
        
        // 等待响应（最长等待 10 分钟，因为用户可能需要较长时间输入）
        let mut buf_reader = BufReader::new(reader);
        let mut response_line = String::new();
        
        let read_result = timeout(
            Duration::from_secs(600),
            buf_reader.read_line(&mut response_line)
        ).await
            .context("等待响应超时")?
            .context("读取响应失败")?;
        
        if read_result == 0 {
            anyhow::bail!("连接已关闭");
        }
        
        // 解析响应
        let response: IpcResponse = serde_json::from_str(response_line.trim())
            .context("解析响应失败")?;
        
        if response.success {
            Ok(response.response)
        } else {
            anyhow::bail!(response.error.unwrap_or_else(|| "未知错误".to_string()))
        }
    }
}
