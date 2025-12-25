// MCP 服务器入口点
use continuum::{mcp::run_server, utils::auto_init_logger, log_important};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 自动初始化日志系统
    auto_init_logger()?;

    log_important!(info, "启动 MCP 服务器");
    run_server().await
}
