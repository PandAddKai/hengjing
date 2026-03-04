import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { ref } from 'vue'

function isWebUiBuild(): boolean {
  return typeof window !== 'undefined' && (window as any).__HENGJING_WEB_BUILD__ === 1
}

/**
 * MCP处理组合式函数
 */
export function useMcpHandler() {
  const mcpRequest = ref(null)
  // Web UI 默认进入“对话/等待态”，避免打开后落到主界面造成误解
  const showMcpPopup = ref(isWebUiBuild())
  const lastRequestId = ref<string | null>(null)

  /**
   * 统一的MCP响应处理
   */
  async function handleMcpResponse(response: any) {
    try {
      // 通过Tauri命令发送响应
      await invoke('send_mcp_response', { response })

      if (isWebUiBuild()) {
        // Web 模式：清除当前请求进入等待状态，不退出
        mcpRequest.value = null
      } else {
        await invoke('exit_app')
      }
    }
    catch (error) {
      console.error('MCP响应处理失败:', error)
    }
  }

  /**
   * 统一的MCP取消处理
   */
  async function handleMcpCancel() {
    try {
      // 发送取消信息
      await invoke('send_mcp_response', { response: 'CANCELLED' })

      if (isWebUiBuild()) {
        // Web 模式：清除当前请求进入等待状态，不退出
        mcpRequest.value = null
      } else {
        await invoke('exit_app')
      }
    }
    catch (error) {
      // 静默处理MCP取消错误
      console.error('MCP取消处理失败:', error)
    }
  }

  /**
   * 显示MCP弹窗
   */
  async function showMcpDialog(request: any) {
    // 获取Telegram配置，检查是否需要隐藏前端弹窗
    let shouldShowFrontendPopup = true
    try {
      const telegramConfig = await invoke('get_telegram_config')
      // 如果Telegram启用且配置了隐藏前端弹窗，则不显示前端弹窗
      if (telegramConfig && (telegramConfig as any).enabled && (telegramConfig as any).hide_frontend_popup) {
        shouldShowFrontendPopup = false
        console.log('🔕 根据Telegram配置，隐藏前端弹窗')
      }
    }
    catch (error) {
      console.error('获取Telegram配置失败:', error)
      // 配置获取失败时，保持默认行为（显示弹窗）
    }

    // 根据配置决定是否显示前端弹窗
    if (shouldShowFrontendPopup) {
      // 设置请求数据和显示状态
      mcpRequest.value = request
      showMcpPopup.value = true
    }
    else {
      console.log('🔕 跳过前端弹窗显示，仅使用Telegram交互')
    }

    // 播放音频通知（无论是否显示弹窗都播放）
    try {
      await invoke('play_notification_sound')
    }
    catch (error) {
      console.error('播放音频通知失败:', error)
    }

    // 启动Telegram同步（无论是否显示弹窗都启动）
    try {
      if (request?.message) {
        await invoke('start_telegram_sync', {
          message: request.message,
          predefinedOptions: request.predefined_options || [],
          isMarkdown: request.is_markdown || false,
        })
        console.log('✅ Telegram同步启动成功')
      }
    }
    catch (error) {
      console.error('启动Telegram同步失败:', error)
    }
  }

  /**
   * 检查MCP模式
   */
  async function checkMcpMode() {
    try {
      const args = await invoke('get_cli_args')

      if (args && (args as any).mcp_request) {
        // 读取MCP请求文件
        const content = await invoke('read_mcp_request', { filePath: (args as any).mcp_request })

        if (content) {
          await showMcpDialog(content)
        }
        return { isMcp: true, mcpContent: content }
      }
    }
    catch (error) {
      console.error('检查MCP模式失败:', error)
    }
    return { isMcp: false, mcpContent: null }
  }

  /**
   * 设置MCP事件监听器
   */
  async function setupMcpEventListener() {
    try {
      const handleEvent = (event: any) => {
        const requestId = event?.payload?.id
        if (requestId && lastRequestId.value === requestId)
          return
        if (requestId)
          lastRequestId.value = requestId
        showMcpDialog(event.payload)
      }

      // 统一使用 mcp-request 事件
      await listen('mcp-request', handleEvent)
    }
    catch (error) {
      console.error('设置MCP事件监听器失败:', error)
    }
  }

  return {
    mcpRequest,
    showMcpPopup,
    handleMcpResponse,
    handleMcpCancel,
    showMcpDialog,
    checkMcpMode,
    setupMcpEventListener,
  }
}
