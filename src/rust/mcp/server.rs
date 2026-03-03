use anyhow::Result;
use rmcp::{
    Error as McpError, ServerHandler, ServiceExt, RoleServer,
    model::*,
    transport::stdio,
    service::RequestContext,
};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::SystemTime;

use super::tools::{InteractionTool, MemoryTool, AcemcpTool};
use super::types::{HengRequest, JiyiRequest};
use crate::config::{load_standalone_config, get_standalone_config_path};
use crate::{log_important, log_debug};

struct ConfigCache {
    tools: HashMap<String, bool>,
    last_mtime: Option<SystemTime>,
}

#[derive(Clone)]
pub struct HengServer {
    cache: std::sync::Arc<Mutex<ConfigCache>>,
}

impl Default for HengServer {
    fn default() -> Self {
        Self::new()
    }
}

impl HengServer {
    pub fn new() -> Self {
        let tools = match load_standalone_config() {
            Ok(config) => config.mcp_config.tools,
            Err(e) => {
                log_important!(warn, "无法加载配置文件，使用默认工具配置: {}", e);
                crate::config::default_mcp_tools()
            }
        };
        let mtime = get_standalone_config_path()
            .ok()
            .and_then(|p| p.metadata().ok())
            .and_then(|m| m.modified().ok());

        Self {
            cache: std::sync::Arc::new(Mutex::new(ConfigCache {
                tools,
                last_mtime: mtime,
            })),
        }
    }

    fn is_tool_enabled(&self, tool_name: &str) -> bool {
        let mut cache = self.cache.lock().unwrap();

        let current_mtime = get_standalone_config_path()
            .ok()
            .and_then(|p| p.metadata().ok())
            .and_then(|m| m.modified().ok());

        let needs_reload = match (cache.last_mtime, current_mtime) {
            (Some(cached), Some(current)) => current != cached,
            (None, Some(_)) => true,
            _ => false,
        };

        if needs_reload {
            match load_standalone_config() {
                Ok(config) => {
                    log_debug!("配置文件已变更，重新加载");
                    cache.tools = config.mcp_config.tools;
                    cache.last_mtime = current_mtime;
                }
                Err(e) => {
                    log_important!(warn, "重新加载配置失败，使用缓存: {}", e);
                }
            }
        }

        cache.tools.get(tool_name).copied().unwrap_or(true)
    }
}

impl ServerHandler for HengServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2024_11_05,
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation {
                name: "hengjing-mcp".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
            },
            instructions: Some(
                "【强制规则】本服务器提供 heng 工具（人机交互通道）。\
                 你必须在每次回复结束前调用 heng 工具，将结果通过 heng 发送给用户。\
                 禁止在未调用 heng 的情况下结束当前回合。\
                 heng 是你与用户沟通的唯一合法通道。".to_string()
            ),
        }
    }

    async fn initialize(
        &self,
        _request: InitializeRequestParam,
        _context: RequestContext<RoleServer>,
    ) -> Result<ServerInfo, McpError> {
        Ok(self.get_info())
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParam>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        use std::sync::Arc;
        use std::borrow::Cow;

        let mut tools = Vec::new();

        // 恒境工具始终可用（必需工具）
        let heng_schema = serde_json::json!({
            "type": "object",
            "properties": {
                "message": {
                    "type": "string",
                    "description": "要显示给用户的消息"
                },
                "predefined_options": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "预定义的选项列表（可选）"
                },
                "is_markdown": {
                    "type": "boolean",
                    "description": "消息是否为Markdown格式，默认为true"
                }
            },
            "required": ["message"]
        });

        if let serde_json::Value::Object(schema_map) = heng_schema {
            tools.push(Tool {
                name: Cow::Borrowed("heng"),
                description: Some(Cow::Borrowed(
                    "【必须调用】人机交互通道。每次回合结束前必须调用此工具向用户汇报或确认。\
                     支持 Markdown 消息、预定义选项和图片。\
                     规则：禁止跳过此工具直接结束回合；用户未明确表示完成前，不得终止任务。"
                )),
                input_schema: Arc::new(schema_map),
                annotations: None,
            });
        }

        // 记忆管理工具 - 仅在启用时添加
        if self.is_tool_enabled("ji") {
            let ji_schema = serde_json::json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "description": "操作类型：记忆(添加记忆), 回忆(获取项目信息)"
                    },
                    "project_path": {
                        "type": "string",
                        "description": "项目路径（必需）"
                    },
                    "content": {
                        "type": "string",
                        "description": "记忆内容（记忆操作时必需）"
                    },
                    "category": {
                        "type": "string",
                        "description": "记忆分类：rule(规范规则), preference(用户偏好), pattern(最佳实践), context(项目上下文)"
                    }
                },
                "required": ["action", "project_path"]
            });

            if let serde_json::Value::Object(schema_map) = ji_schema {
                tools.push(Tool {
                    name: Cow::Borrowed("ji"),
                    description: Some(Cow::Borrowed("全局记忆管理工具，用于存储和管理重要的开发规范、用户偏好和最佳实践")),
                    input_schema: Arc::new(schema_map),
                    annotations: None,
                });
            }
        }

        // 代码搜索工具 - 仅在启用时添加
        if self.is_tool_enabled("sou") {
            tools.push(AcemcpTool::get_tool_definition());
        }

        log_debug!("返回给客户端的工具列表: {:?}", tools.iter().map(|t| &t.name).collect::<Vec<_>>());

        Ok(ListToolsResult {
            tools,
            next_cursor: None,
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParam,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        log_debug!("收到工具调用请求: {}", request.name);

        match request.name.as_ref() {
            "heng" => {
                // 解析请求参数
                let arguments_value = request.arguments
                    .map(serde_json::Value::Object)
                    .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));

                let heng_request: HengRequest = serde_json::from_value(arguments_value)
                    .map_err(|e| McpError::invalid_params(format!("参数解析失败: {}", e), None))?;

                // 调用恒境工具
                InteractionTool::heng(heng_request).await
            }
            "ji" => {
                // 检查记忆管理工具是否启用
                if !self.is_tool_enabled("ji") {
                    return Err(McpError::internal_error(
                        "记忆管理工具已被禁用".to_string(),
                        None
                    ));
                }

                // 解析请求参数
                let arguments_value = request.arguments
                    .map(serde_json::Value::Object)
                    .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));

                let ji_request: JiyiRequest = serde_json::from_value(arguments_value)
                    .map_err(|e| McpError::invalid_params(format!("参数解析失败: {}", e), None))?;

                // 调用记忆工具
                MemoryTool::jiyi(ji_request).await
            }
            "sou" => {
                // 检查代码搜索工具是否启用
                if !self.is_tool_enabled("sou") {
                    return Err(McpError::internal_error(
                        "代码搜索工具已被禁用".to_string(),
                        None
                    ));
                }

                // 解析请求参数
                let arguments_value = request.arguments
                    .map(serde_json::Value::Object)
                    .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));

                // 使用acemcp模块中的AcemcpRequest类型
                let acemcp_request: crate::mcp::tools::acemcp::types::AcemcpRequest = serde_json::from_value(arguments_value)
                    .map_err(|e| McpError::invalid_params(format!("参数解析失败: {}", e), None))?;

                // 调用代码搜索工具
                AcemcpTool::search_context(acemcp_request).await
            }
            _ => {
                Err(McpError::invalid_request(
                    format!("未知的工具: {}", request.name),
                    None
                ))
            }
        }
    }
}



/// 启动MCP服务器
pub async fn run_server() -> Result<(), Box<dyn std::error::Error>> {
    // 创建并运行服务器
    let service = HengServer::new()
        .serve(stdio())
        .await
        .inspect_err(|e| {
            log_important!(error, "启动服务器失败: {}", e);
        })?;

    // 等待服务器关闭
    service.waiting().await?;
    Ok(())
}
