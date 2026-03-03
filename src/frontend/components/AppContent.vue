<script setup lang="ts">
import { invoke } from '@tauri-apps/api/core'
import { useMessage } from 'naive-ui'
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { setupExitWarningListener } from '../composables/useExitWarning'
import { useKeyboard } from '../composables/useKeyboard'
import { useVersionCheck } from '../composables/useVersionCheck'
import UpdateModal from './common/UpdateModal.vue'
import LayoutWrapper from './layout/LayoutWrapper.vue'
import McpPopup from './popup/McpPopup.vue'
import PopupHeader from './popup/PopupHeader.vue'

interface AppConfig {
  theme: string
  window: {
    alwaysOnTop: boolean
    width: number
    height: number
    fixed: boolean
  }
  audio: {
    enabled: boolean
    url: string
  }
  reply: {
    enabled: boolean
    prompt: string
  }
}

interface Props {
  mcpRequest: any
  showMcpPopup: boolean
  appConfig: AppConfig
  isInitializing: boolean
}

interface Emits {
  mcpResponse: [response: any]
  mcpCancel: []
  themeChange: [theme: string]
  toggleAlwaysOnTop: []
  toggleAudioNotification: []
  updateAudioUrl: [url: string]
  testAudio: []
  stopAudio: []
  testAudioError: [error: any]
  updateWindowSize: [size: { width: number, height: number, fixed: boolean }]
  updateReplyConfig: [config: { enable_continue_reply?: boolean, continue_prompt?: string }]
  messageReady: [message: any]
  configReloaded: []
}

const props = defineProps<Props>()
const emit = defineEmits<Emits>()

// 版本检查相关
const { versionInfo, showUpdateModal } = useVersionCheck()

// 弹窗中的设置显示控制
const showPopupSettings = ref(false)

// Web 模式检测
const isWebMode = computed(() => typeof window !== 'undefined' && (window as any).__HENGJING_WEB_BUILD__ === 1)

// 交互历史记录
const interactionHistory = ref<any[]>([])

// 加载交互历史
async function loadInteractionHistory() {
  try {
    const history = await invoke('get_interaction_history')
    interactionHistory.value = (history as any[]) || []
  }
  catch {
    interactionHistory.value = []
  }
}

// 截断文本
function truncateText(text: string, maxLen: number): string {
  if (!text) return ''
  return text.length > maxLen ? text.substring(0, maxLen) + '...' : text
}

// 当进入等待状态时加载交互历史
watch(() => props.mcpRequest, (newReq, oldReq) => {
  if (!newReq && oldReq && props.showMcpPopup && isWebMode.value) {
    loadInteractionHistory()
  }
})

// 初始化 Naive UI 消息实例
const message = useMessage()

// 键盘快捷键处理
const { handleExitShortcut } = useKeyboard()

// 切换弹窗设置显示
function togglePopupSettings() {
  showPopupSettings.value = !showPopupSettings.value
}

// 手动处理窗口拖拽
async function startWindowDrag(event: MouseEvent | TouchEvent) {
  // 如果是按钮等交互元素，不触发拖拽
  const target = event.target as HTMLElement
  if (target.closest('button') || target.closest('[role="button"]') || target.closest('a')) {
    return
  }
  
  // 阻止默认行为（如文本选择）
  event.preventDefault()
  
  try {
    const appWindow = getCurrentWindow()
    await appWindow.startDragging()
  } catch (error) {
    console.error('Failed to start dragging:', error)
  }
}

// 监听 MCP 请求变化，当有新请求时重置设置页面状态
watch(() => props.mcpRequest, (newRequest) => {
  if (newRequest && showPopupSettings.value) {
    // 有新的 MCP 请求时，自动切换回消息页面
    showPopupSettings.value = false
  }
}, { immediate: true })

// 全局键盘事件处理器
function handleGlobalKeydown(event: KeyboardEvent) {
  handleExitShortcut(event)
}

onMounted(() => {
  // 将消息实例传递给父组件
  emit('messageReady', message)
  // 设置退出警告监听器（统一处理主界面和弹窗）
  setupExitWarningListener(message)

  // 添加全局键盘事件监听器
  document.addEventListener('keydown', handleGlobalKeydown)
})

onUnmounted(() => {
  // 移除键盘事件监听器
  document.removeEventListener('keydown', handleGlobalKeydown)
})
</script>

<template>
  <div class="min-h-screen bg-black">
    <!-- MCP弹窗模式 -->
    <div
      v-if="props.showMcpPopup && props.mcpRequest"
      class="flex flex-col w-full h-screen bg-black text-white select-none"
    >
      <!-- 头部 - 固定在顶部，支持拖拽 -->
      <div 
        class="sticky top-0 z-50 flex-shrink-0 bg-black-100 border-b-2 border-black-200 pt-8" 
        style="-webkit-user-select: none; user-select: none; cursor: default;"
        @mousedown="startWindowDrag"
        @touchstart="startWindowDrag"
      >
        <PopupHeader
          :current-theme="props.appConfig.theme"
          :loading="false"
          :show-main-layout="showPopupSettings"
          :always-on-top="props.appConfig.window.alwaysOnTop"
          @theme-change="$emit('themeChange', $event)"
          @open-main-layout="togglePopupSettings"
          @toggle-always-on-top="$emit('toggleAlwaysOnTop')"
        />
      </div>

      <!-- 设置界面 -->
      <div
        v-show="showPopupSettings"
        class="flex-1 overflow-y-auto scrollbar-thin"
      >
        <LayoutWrapper
          :app-config="props.appConfig"
          @theme-change="$emit('themeChange', $event)"
          @toggle-always-on-top="$emit('toggleAlwaysOnTop')"
          @toggle-audio-notification="$emit('toggleAudioNotification')"
          @update-audio-url="$emit('updateAudioUrl', $event)"
          @test-audio="$emit('testAudio')"
          @stop-audio="$emit('stopAudio')"
          @test-audio-error="$emit('testAudioError', $event)"
          @update-window-size="$emit('updateWindowSize', $event)"
        />
      </div>

      <!-- 弹窗内容 -->
      <McpPopup
        v-show="!showPopupSettings"
        :request="props.mcpRequest"
        :app-config="props.appConfig"
        @response="$emit('mcpResponse', $event)"
        @cancel="$emit('mcpCancel')"
        @theme-change="$emit('themeChange', $event)"
      />
    </div>

    <!-- Web 模式等待下一个请求 -->
    <div
      v-else-if="props.showMcpPopup && !props.mcpRequest && isWebMode"
      class="flex flex-col w-full h-screen bg-black text-white"
    >
      <div class="flex-1 flex flex-col items-center justify-center p-8">
        <!-- 等待动画 -->
        <div class="relative mb-6">
          <div class="w-16 h-16 rounded-full bg-primary-500/10 flex items-center justify-center">
            <div class="w-8 h-8 rounded-full bg-primary-500/20 animate-pulse" />
          </div>
        </div>
        <div class="text-gray-400 text-sm mb-2">已提交，等待下一个请求...</div>
        <div class="text-gray-600 text-xs mb-8">页面将自动刷新</div>

        <!-- 最近交互记录 -->
        <div v-if="interactionHistory.length > 0" class="w-full max-w-2xl">
          <div class="text-xs text-gray-500 uppercase tracking-wider mb-3 text-center">最近交互记录</div>
          <div class="space-y-2">
            <div
              v-for="(item, index) in interactionHistory"
              :key="index"
              class="bg-gray-900/50 border border-gray-800 rounded-lg p-3 text-xs"
            >
              <div class="flex justify-between items-center mb-2">
                <span class="text-gray-500 font-mono">{{ item.timestamp }}</span>
                <span class="text-gray-700 font-mono">#{{ (item.id || '').substring(0, 8) }}</span>
              </div>
              <div class="text-gray-400 mb-1">
                <span class="text-gray-600 mr-1">请求:</span>{{ truncateText(item.message, 120) }}
              </div>
              <div class="text-green-400/80">
                <span class="text-gray-600 mr-1">响应:</span>{{ truncateText(item.response, 120) }}
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>

    <!-- 弹窗加载骨架屏 或 初始化骨架屏 -->
    <div
      v-else-if="(props.showMcpPopup && !isWebMode) || props.isInitializing"
      class="flex flex-col w-full h-screen bg-black text-white"
    >
      <!-- 头部骨架 - 支持拖拽 -->
      <div 
        class="flex-shrink-0 bg-black-100 border-b-2 border-black-200 px-4 py-3 pt-11"
        style="-webkit-user-select: none; user-select: none; cursor: default;"
        @mousedown="startWindowDrag"
        @touchstart="startWindowDrag"
      >
        <div class="flex items-center justify-between pointer-events-none">
          <div class="flex items-center gap-3">
            <n-skeleton
              circle
              :width="12"
              :height="12"
            />
            <n-skeleton
              text
              :width="256"
            />
          </div>
          <div class="flex gap-2">
            <n-skeleton
              circle
              :width="32"
              :height="32"
            />
            <n-skeleton
              circle
              :width="32"
              :height="32"
            />
          </div>
        </div>
      </div>

      <!-- 内容骨架 -->
      <div class="flex-1 p-4">
        <div class="bg-black-100 rounded-lg p-4 mb-4">
          <n-skeleton
            text
            :repeat="3"
          />
        </div>

        <div class="space-y-3">
          <n-skeleton
            text
            :width="128"
          />
          <n-skeleton
            text
            :repeat="3"
          />
        </div>
      </div>

      <!-- 底部骨架 -->
      <div class="flex-shrink-0 bg-black-100 border-t-2 border-black-200 p-4">
        <div class="flex justify-between items-center">
          <n-skeleton
            text
            :width="96"
          />
          <div class="flex gap-2">
            <n-skeleton
              text
              :width="64"
              :height="32"
            />
            <n-skeleton
              text
              :width="64"
              :height="32"
            />
          </div>
        </div>
      </div>
    </div>

    <!-- 主界面 - 只在非弹窗模式且非初始化时显示 -->
    <LayoutWrapper
      v-else
      :app-config="props.appConfig"
      @theme-change="$emit('themeChange', $event)"
      @toggle-always-on-top="$emit('toggleAlwaysOnTop')"
      @toggle-audio-notification="$emit('toggleAudioNotification')"
      @update-audio-url="$emit('updateAudioUrl', $event)"
      @test-audio="$emit('testAudio')"
      @stop-audio="$emit('stopAudio')"
      @test-audio-error="$emit('testAudioError', $event)"
      @update-window-size="$emit('updateWindowSize', $event)"
      @config-reloaded="$emit('configReloaded')"
    />

    <!-- 更新弹窗 -->
    <UpdateModal
      v-model:show="showUpdateModal"
      :version-info="versionInfo"
    />
  </div>
</template>
