use anyhow::Result;
use std::fs;
use std::path::PathBuf;
use tauri::{AppHandle, LogicalSize, Manager, State};

use super::settings::{default_shortcuts, AppConfig, AppState, CURRENT_CONFIG_VERSION};

pub fn get_config_path(_app: &AppHandle) -> Result<PathBuf> {
    get_standalone_config_path()
}

pub async fn save_config(state: &State<'_, AppState>, app: &AppHandle) -> Result<()> {
    let config_path = get_config_path(app)?;

    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let config = state
        .config
        .lock()
        .map_err(|e| anyhow::anyhow!("获取配置失败: {}", e))?;
    let config_json = serde_json::to_string_pretty(&*config)?;

    fs::write(&config_path, config_json)?;

    if let Ok(file) = std::fs::OpenOptions::new().write(true).open(&config_path) {
        let _ = file.sync_all();
    }

    log::debug!("配置已保存到: {:?}", config_path);

    Ok(())
}

/// Tauri应用专用的配置加载函数
pub async fn load_config(state: &State<'_, AppState>, app: &AppHandle) -> Result<()> {
    let config_path = get_config_path(app)?;

    if config_path.exists() {
        let config_json = fs::read_to_string(&config_path)?;
        let mut config: AppConfig = serde_json::from_str(&config_json)?;

        run_migrations(&mut config);

        let mut config_guard = state
            .config
            .lock()
            .map_err(|e| anyhow::anyhow!("获取配置锁失败: {}", e))?;
        *config_guard = config;
    }

    Ok(())
}

pub async fn load_config_and_apply_window_settings(
    state: &State<'_, AppState>,
    app: &AppHandle,
) -> Result<()> {
    load_config(state, app).await?;

    let (always_on_top, window_config) = {
        let config = state
            .config
            .lock()
            .map_err(|e| anyhow::anyhow!("获取配置失败: {}", e))?;
        (
            config.ui_config.always_on_top,
            config.ui_config.window_config.clone(),
        )
    };

    if let Some(window) = app.get_webview_window("main") {
        if let Err(e) = window.set_always_on_top(always_on_top) {
            log::warn!("设置窗口置顶失败: {}", e);
        } else {
            log::info!("窗口置顶状态已设置为: {} (配置加载时)", always_on_top);
        }

        if let Err(e) = window.set_min_size(Some(LogicalSize::new(
            window_config.min_width,
            window_config.min_height,
        ))) {
            log::warn!("设置最小窗口大小失败: {}", e);
        }

        if let Err(e) = window.set_max_size(Some(LogicalSize::new(
            window_config.max_width,
            window_config.max_height,
        ))) {
            log::warn!("设置最大窗口大小失败: {}", e);
        }

        let (target_width, target_height) = if window_config.fixed {
            (window_config.fixed_width, window_config.fixed_height)
        } else {
            (window_config.free_width, window_config.free_height)
        };

        if let Err(_e) = window.set_size(LogicalSize::new(target_width, target_height)) {}
    }

    Ok(())
}

/// 独立加载配置文件（用于MCP服务器等独立进程）
pub fn load_standalone_config() -> Result<AppConfig> {
    let config_path = get_standalone_config_path()?;

    if config_path.exists() {
        let config_json = fs::read_to_string(config_path)?;
        let mut config: AppConfig = serde_json::from_str(&config_json)?;

        run_migrations(&mut config);

        Ok(config)
    } else {
        Ok(AppConfig::default())
    }
}

/// 独立加载Telegram配置（用于MCP模式下的配置检查）
pub fn load_standalone_telegram_config() -> Result<super::settings::TelegramConfig> {
    let config = load_standalone_config()?;
    Ok(config.telegram_config)
}

/// 独立保存配置文件（用于Web模式等独立进程）
pub fn save_standalone_config(config: &AppConfig) -> Result<()> {
    let config_path = get_standalone_config_path()?;
    let config_json = serde_json::to_string_pretty(config)?;
    fs::write(&config_path, config_json)?;
    Ok(())
}

/// 获取独立配置文件路径（不依赖Tauri）
pub fn get_standalone_config_path() -> Result<PathBuf> {
    let config_dir = dirs::config_dir()
        .ok_or_else(|| anyhow::anyhow!("无法获取配置目录"))?
        .join("continuum");

    fs::create_dir_all(&config_dir)?;

    Ok(config_dir.join("config.json"))
}

/// 运行所有需要的配置迁移
///
/// 按版本号顺序执行迁移，最后将 config_version 更新到最新。
/// 新增迁移时只需在此函数底部添加新的 `if from_version < N` 分支。
fn run_migrations(config: &mut AppConfig) {
    let from_version = config.config_version;

    if from_version < 1 {
        migrate_v0_to_v1(config);
    }

    config.config_version = CURRENT_CONFIG_VERSION;
}

/// v0 -> v1: 合并默认快捷键，修正 enhance 快捷键默认值
fn migrate_v0_to_v1(config: &mut AppConfig) {
    let defaults = default_shortcuts();
    for (key, default_binding) in defaults {
        if !config.shortcut_config.shortcuts.contains_key(&key) {
            config
                .shortcut_config
                .shortcuts
                .insert(key, default_binding);
        } else if key == "enhance" {
            let existing = config.shortcut_config.shortcuts.get(&key).unwrap();
            if existing.key_combination.key == "Enter"
                && !existing.key_combination.ctrl
                && existing.key_combination.shift
                && !existing.key_combination.alt
                && !existing.key_combination.meta
            {
                config
                    .shortcut_config
                    .shortcuts
                    .insert(key, default_binding);
            }
        }
    }
}
