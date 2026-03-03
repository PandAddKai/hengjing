use hengjing::app::{handle_cli_args, run_tauri_app};
use hengjing::mcp::run_server;
use hengjing::utils::auto_init_logger;
use hengjing::log_important;
use anyhow::Result;

fn main() -> Result<()> {
    if let Err(e) = auto_init_logger() {
        eprintln!("初始化日志系统失败: {}", e);
    }

    let args: Vec<String> = std::env::args().collect();
    let exe_name = std::path::Path::new(&args[0])
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");

    if exe_name.contains("恒境") || exe_name.contains("hengjing") {
        return run_mcp_server();
    }

    if args.len() >= 2 {
        match args[1].as_str() {
            "gui" => {
                run_tauri_app();
                return Ok(());
            }
            "serve" => return run_mcp_server(),
            _ => {}
        }
    }

    handle_cli_args()
}

fn run_mcp_server() -> Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        log_important!(info, "启动 MCP 服务器");
        run_server().await.map_err(|e| anyhow::anyhow!("{}", e))
    })
}
