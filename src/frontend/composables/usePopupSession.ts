import type { McpRequest } from '../types/popup'
import { invoke } from '@tauri-apps/api/core'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { ref } from 'vue'

export function usePopupSession() {
  const mcpRequest = ref<McpRequest | null>(null)
  const showMcpPopup = ref(true)

  async function loadCurrentRequest() {
    try {
      const request = await invoke<McpRequest | null>('get_popup_request_for_current_window')
      if (!request) {
        throw new Error('当前 popup 未绑定请求')
      }
      mcpRequest.value = request
      return request
    }
    catch (error) {
      console.error('加载 popup 请求失败:', error)
      await getCurrentWebviewWindow().destroy()
      throw error
    }
  }

  async function handleMcpResponse(response: any) {
    const currentRequest = mcpRequest.value
    if (!currentRequest?.id) {
      throw new Error('当前 popup 没有可响应的请求')
    }

    invoke('save_conversation_record', {
      request: currentRequest,
      response,
    }).catch(error => console.error('保存对话记录失败:', error))

    await invoke('send_popup_response', {
      requestId: currentRequest.id,
      response,
    })
    mcpRequest.value = null
  }

  async function handleMcpCancel() {
    const currentRequest = mcpRequest.value
    if (!currentRequest?.id) {
      throw new Error('当前 popup 没有可取消的请求')
    }

    await invoke('cancel_popup_request', {
      requestId: currentRequest.id,
    })
    mcpRequest.value = null
  }

  return {
    mcpRequest,
    showMcpPopup,
    loadCurrentRequest,
    handleMcpResponse,
    handleMcpCancel,
  }
}
