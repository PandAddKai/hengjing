use crate::config::load_standalone_telegram_config;
use crate::telegram::handle_telegram_only_mcp_request;
use crate::web::server::{handle_web_mode, should_use_web_mode};
use crate::mcp::types::PopupRequest;
use crate::log_important;
use crate::app::builder::run_tauri_app;
use anyhow::Result;

/// 处理命令行参数
pub fn handle_cli_args() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();

    match args.len() {
        // 无参数：正常启动GUI
        1 => {
            run_tauri_app();
        }
        // 单参数：帮助或版本
        2 => {
            match args[1].as_str() {
                "--help" | "-h" => print_help(),
                "--version" | "-v" => print_version(),
                _ => {
                    eprintln!("未知参数: {}", args[1]);
                    print_help();
                    std::process::exit(1);
                }
            }
        }
        // 多参数：MCP请求模式
        _ => {
            if args[1] == "--mcp-request" && args.len() >= 3 {
                handle_mcp_request(&args[2])?;
            } else {
                eprintln!("无效的命令行参数");
                print_help();
                std::process::exit(1);
            }
        }
    }

    Ok(())
}

/// 处理MCP请求
fn handle_mcp_request(request_file: &str) -> Result<()> {
    // 1. 检查Telegram配置，决定是否启用纯Telegram模式
    match load_standalone_telegram_config() {
        Ok(telegram_config) => {
            if telegram_config.enabled && telegram_config.hide_frontend_popup {
                // 纯Telegram模式：不启动GUI，直接处理
                if let Err(e) = tokio::runtime::Runtime::new()
                    .unwrap()
                    .block_on(handle_telegram_only_mcp_request(request_file))
                {
                    log_important!(error, "处理Telegram请求失败: {}", e);
                    std::process::exit(1);
                }
                return Ok(());
            }
        }
        Err(e) => {
            log_important!(warn, "加载Telegram配置失败: {}，继续检测环境", e);
        }
    }

    // 2. 检测是否需要 Web 模式（无图形环境）
    if should_use_web_mode() {
        log_important!(info, "检测到无图形环境，启动 Web 模式");
        return handle_web_mcp_request(request_file);
    }

    // 3. 正常模式：启动GUI处理弹窗
    run_tauri_app();
    Ok(())
}

/// Web 模式处理 MCP 请求
fn handle_web_mcp_request(request_file: &str) -> Result<()> {
    // 读取请求文件
    let request_json = std::fs::read_to_string(request_file)?;
    let request: PopupRequest = serde_json::from_str(&request_json)?;

    let rt = tokio::runtime::Runtime::new()?;
    let result = rt.block_on(handle_web_mode(&request))?;

    // 输出响应到 stdout（MCP 协议要求）
    println!("{}", result);

    Ok(())
}

/// 显示帮助信息
fn print_help() {
    println!("恒境 - 智能代码审查工具 v{}", env!("CARGO_PKG_VERSION"));
    println!();
    println!("用法:");
    println!("  hengjing gui              启动设置界面（GUI）");
    println!("  hengjing serve            启动 MCP 服务器（stdio）");
    println!("  hengjing --mcp-request <文件>  处理单个 MCP 请求");
    println!("  hengjing --help           显示此帮助信息");
    println!("  hengjing --version        显示版本信息");
    println!();
    println!("兼容命令:");
    println!("  等                        等同于 hengjing gui");
    println!("  恒境                      等同于 hengjing serve");
    println!();
    println!("环境变量:");
    println!("  HENGJING_WEB_MODE=1       强制使用 Web 模式");
    println!("  HENGJING_WEB_PORT=18963   Web 模式端口（默认 18963）");
    println!("  HENGJING_WEB_HOST=0.0.0.0 Web 模式监听地址（默认 127.0.0.1）");
}

/// 显示版本信息
fn print_version() {
    println!("恒境 v{}", env!("CARGO_PKG_VERSION"));
}
