<script setup lang="ts">
import type { McpRequest } from '../../types/popup'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { useMessage } from 'naive-ui'
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'

import PopupActions from './PopupActions.vue'
import PopupContent from './PopupContent.vue'
import PopupInput from './PopupInput.vue'

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
  request: McpRequest | null
  appConfig: AppConfig
  mockMode?: boolean
  testMode?: boolean
}

interface Emits {
  response: [response: any]
  cancel: []
  themeChange: [theme: string]
  openMainLayout: []
  toggleAlwaysOnTop: []
  toggleAudioNotification: []
  updateAudioUrl: [url: string]
  testAudio: []
  stopAudio: []
  testAudioError: [error: any]
  updateWindowSize: [size: { width: number, height: number, fixed: boolean }]
}

const props = withDefaults(defineProps<Props>(), {
  mockMode: false,
  testMode: false,
})

const emit = defineEmits<Emits>()

// 使用消息提示
const message = useMessage()

// 响应式状态
const loading = ref(false)
const submitting = ref(false)
const selectedOptions = ref<string[]>([])
const userInput = ref('')
const draggedImages = ref<string[]>([])
const inputRef = ref()

// 继续回复配置
const continueReplyEnabled = ref(true)
const continuePrompt = ref('请按照最佳实践继续')

// 超时自动提交状态
const timeoutAutoSubmitEnabled = ref(false)
const timeoutSeconds = ref(360)
const timeoutPromptSource = ref('continue')
const timeoutCustomPromptId = ref<string | null>(null)
const timeoutManualPrompt = ref('请按照最佳实践继续')
const countdownRemaining = ref(0)
const countdownTimer = ref<ReturnType<typeof setInterval> | null>(null)

// 计算属性
const isVisible = computed(() => !!props.request)
const hasOptions = computed(() => (props.request?.predefined_options?.length ?? 0) > 0)
const canSubmit = computed(() => {
  if (hasOptions.value) {
    return selectedOptions.value.length > 0 || userInput.value.trim().length > 0 || draggedImages.value.length > 0
  }
  return userInput.value.trim().length > 0 || draggedImages.value.length > 0
})

// 获取输入组件的状态文本
const inputStatusText = computed(() => {
  return inputRef.value?.statusText || '等待输入...'
})

// 加载继续回复配置
async function loadReplyConfig() {
  try {
    const config = await invoke('get_reply_config')
    if (config) {
      const replyConfig = config as any
      continueReplyEnabled.value = replyConfig.enable_continue_reply ?? true
      continuePrompt.value = replyConfig.continue_prompt ?? '请按照最佳实践继续'
    }
  }
  catch (error) {
    console.log('加载继续回复配置失败，使用默认值:', error)
  }
}

// 加载超时自动提交配置
async function loadTimeoutAutoSubmitConfig() {
  try {
    const config = await invoke('get_timeout_auto_submit_config') as any
    if (config) {
      timeoutAutoSubmitEnabled.value = config.enabled ?? false
      timeoutSeconds.value = config.timeout_seconds ?? 360
      timeoutPromptSource.value = config.prompt_source ?? 'continue'
      timeoutCustomPromptId.value = config.custom_prompt_id ?? null
      timeoutManualPrompt.value = config.manual_prompt ?? '请按照最佳实践继续'
    }
  }
  catch (error) {
    console.log('加载超时自动提交配置失败，使用默认值:', error)
  }
}

// 开始倒计时
function startCountdown() {
  stopCountdown()
  if (!timeoutAutoSubmitEnabled.value || timeoutSeconds.value <= 0)
    return

  countdownRemaining.value = timeoutSeconds.value
  countdownTimer.value = setInterval(() => {
    countdownRemaining.value--
    if (countdownRemaining.value <= 0) {
      stopCountdown()
      handleTimeoutAutoSubmit()
    }
  }, 1000)
}

// 停止倒计时
function stopCountdown() {
  if (countdownTimer.value) {
    clearInterval(countdownTimer.value)
    countdownTimer.value = null
  }
  countdownRemaining.value = 0
}

// 取消倒计时（语义别名）
function cancelCountdown() {
  stopCountdown()
}

// 超时自动提交处理
async function handleTimeoutAutoSubmit() {
  if (submitting.value)
    return

  submitting.value = true

  try {
    let promptContent = ''

    if (timeoutPromptSource.value === 'continue') {
      promptContent = continuePrompt.value
    }
    else if (timeoutPromptSource.value === 'recall_heng') {
      promptContent = '重新调用且慢MCP工具'
    }
    else if (timeoutPromptSource.value === 'custom' && timeoutCustomPromptId.value) {
      try {
        const config = await invoke('get_custom_prompt_config') as any
        const prompt = config?.prompts?.find((p: any) => p.id === timeoutCustomPromptId.value)
        if (prompt) {
          promptContent = prompt.content
        }
        else {
          promptContent = continuePrompt.value
        }
      }
      catch {
        promptContent = continuePrompt.value
      }
    }
    else if (timeoutPromptSource.value === 'manual') {
      promptContent = timeoutManualPrompt.value
    }
    else {
      promptContent = continuePrompt.value
    }

    const response = {
      user_input: promptContent,
      selected_options: [],
      images: [],
      metadata: {
        timestamp: new Date().toISOString(),
        request_id: props.request?.id || null,
        source: 'popup_timeout_auto_submit',
      },
    }

    emit('response', response)
    // 不重置 submitting — emit 后父组件会处理响应并关闭窗口
    // 保持 submitting=true 防止用户在关闭前再次操作
  }
  catch (error) {
    console.error('超时自动提交失败:', error)
    message.error('超时自动提交失败')
    submitting.value = false
  }
}

// 监听配置变化（当从设置页面切换回来时）
watch(() => props.appConfig.reply, (newReplyConfig) => {
  if (newReplyConfig) {
    continueReplyEnabled.value = newReplyConfig.enabled
    continuePrompt.value = newReplyConfig.prompt
  }
}, { deep: true, immediate: true })

// Telegram事件监听器
let telegramUnlisten: (() => void) | null = null

// 监听请求变化
watch(() => props.request, async (newRequest) => {
  if (newRequest) {
    resetForm()
    loading.value = true
    // 每次显示弹窗时重新加载配置
    await loadReplyConfig()
    await loadTimeoutAutoSubmitConfig()
    setTimeout(() => {
      loading.value = false
      // loading 结束后开始倒计时
      startCountdown()
    }, 300)
  }
  else {
    stopCountdown()
  }
}, { immediate: true })

// 设置Telegram事件监听
async function setupTelegramListener() {
  try {
    telegramUnlisten = await listen('telegram-event', (event) => {
      console.log('🎯 [McpPopup] 收到Telegram事件:', event)
      console.log('🎯 [McpPopup] 事件payload:', event.payload)
      handleTelegramEvent(event.payload as any)
    })
    console.log('🎯 [McpPopup] Telegram事件监听器已设置')
  }
  catch (error) {
    console.error('🎯 [McpPopup] 设置Telegram事件监听器失败:', error)
  }
}

// 处理Telegram事件
function handleTelegramEvent(event: any) {
  console.log('🎯 [McpPopup] 开始处理事件:', event.type)

  switch (event.type) {
    case 'option_toggled':
      console.log('🎯 [McpPopup] 处理选项切换:', event.option)
      handleOptionToggle(event.option)
      break
    case 'text_updated':
      console.log('🎯 [McpPopup] 处理文本更新:', event.text)
      handleTextUpdate(event.text)
      break
    case 'continue_pressed':
      console.log('🎯 [McpPopup] 处理继续按钮')
      handleContinue()
      break
    case 'send_pressed':
      console.log('🎯 [McpPopup] 处理发送按钮')
      handleSubmit()
      break
    default:
      console.log('🎯 [McpPopup] 未知事件类型:', event.type)
  }
}

// 处理选项切换
function handleOptionToggle(option: string) {
  const index = selectedOptions.value.indexOf(option)
  if (index > -1) {
    // 取消选择
    selectedOptions.value.splice(index, 1)
  }
  else {
    // 添加选择
    selectedOptions.value.push(option)
  }

  // 同步到PopupInput组件
  if (inputRef.value) {
    inputRef.value.updateData({ selectedOptions: selectedOptions.value })
  }
}

// 处理文本更新
function handleTextUpdate(text: string) {
  userInput.value = text

  // 同步到PopupInput组件
  if (inputRef.value) {
    inputRef.value.updateData({ userInput: text })
  }
}

// 组件挂载时设置监听器和加载配置
onMounted(() => {
  loadReplyConfig()
  setupTelegramListener()
})

// 组件卸载时清理监听器
onUnmounted(() => {
  stopCountdown()
  if (telegramUnlisten) {
    telegramUnlisten()
  }
})

// 重置表单
function resetForm() {
  selectedOptions.value = []
  userInput.value = ''
  draggedImages.value = []
  submitting.value = false
}

// 处理提交
async function handleSubmit() {
  if (!canSubmit.value || submitting.value)
    return

  stopCountdown()
  submitting.value = true

  try {
    // 使用新的结构化数据格式
    const response = {
      user_input: userInput.value.trim() || null,
      selected_options: selectedOptions.value,
      images: draggedImages.value.map(imageData => ({
        data: imageData.split(',')[1], // 移除 data:image/png;base64, 前缀
        media_type: 'image/png',
        filename: null,
      })),
      metadata: {
        timestamp: new Date().toISOString(),
        request_id: props.request?.id || null,
        source: 'popup',
      },
    }

    // 如果没有任何有效内容，设置默认用户输入
    if (!response.user_input && response.selected_options.length === 0 && response.images.length === 0) {
      response.user_input = '用户确认继续'
    }

    if (props.mockMode) {
      // 模拟模式下的延迟
      await new Promise(resolve => setTimeout(resolve, 1000))
      message.success('模拟响应发送成功')
    }

    // 通过事件触发统一的响应处理（send_mcp_response + exit_app）
    // 注意：不再在这里直接调用 invoke，避免重复发送
    emit('response', response)
  }
  catch (error) {
    console.error('提交响应失败:', error)
    message.error('提交失败，请重试')
  }
  finally {
    submitting.value = false
  }
}

// 处理输入更新
function handleInputUpdate(data: { userInput: string, selectedOptions: string[], draggedImages: string[] }) {
  userInput.value = data.userInput
  selectedOptions.value = data.selectedOptions
  draggedImages.value = data.draggedImages
}

// 处理图片添加 - 移除重复逻辑，避免双重添加
function handleImageAdd(_image: string) {
  // 这个函数现在只是为了保持接口兼容性，实际添加在PopupInput中完成
}

// 处理图片移除
function handleImageRemove(index: number) {
  draggedImages.value.splice(index, 1)
}

// 处理继续按钮点击
async function handleContinue() {
  if (submitting.value)
    return

  stopCountdown()
  submitting.value = true

  try {
    // 使用新的结构化数据格式
    const response = {
      user_input: continuePrompt.value,
      selected_options: [],
      images: [],
      metadata: {
        timestamp: new Date().toISOString(),
        request_id: props.request?.id || null,
        source: 'popup_continue',
      },
    }

    if (props.mockMode) {
      // 模拟模式下的延迟
      await new Promise(resolve => setTimeout(resolve, 1000))
      message.success('继续请求发送成功')
    }

    // 通过事件触发统一的响应处理（send_mcp_response + exit_app）
    // 注意：不再在这里直接调用 invoke，避免重复发送
    emit('response', response)
  }
  catch (error) {
    console.error('发送继续请求失败:', error)
    message.error('继续请求失败，请重试')
  }
  finally {
    submitting.value = false
  }
}

// 处理引用消息
function handleQuoteMessage(messageContent: string) {
  if (inputRef.value) {
    inputRef.value.handleQuoteMessage(messageContent)
  }
}

// 处理增强按钮点击
async function handleEnhance() {
  if (submitting.value)
    return

  stopCountdown()
  submitting.value = true

  try {
    // 构建增强prompt
    const enhancePrompt = `Use the following prompt to optimize and enhance the context of the content in 《》, and return the enhanced result by calling the tool '且慢' after completion.Here is an instruction that I'd like to give you, but it needs to be improved. Rewrite and enhance this instruction to make it clearer, more specific, less ambiguous, and correct any mistakes. Reply immediately with your answer, even if you're not sure. Consider the context of our conversation history when enhancing the prompt. Reply with the following format:

### BEGIN RESPONSE ###
Here is an enhanced version of the original instruction that is more specific and clear:
<augment-enhanced-prompt>enhanced prompt goes here</augment-enhanced-prompt>

### END RESPONSE ###

Here is my original instruction:

《${userInput.value.trim()}》`

    // 使用新的结构化数据格式
    const response = {
      user_input: enhancePrompt,
      selected_options: [],
      images: [],
      metadata: {
        timestamp: new Date().toISOString(),
        request_id: props.request?.id || null,
        source: 'popup_enhance',
      },
    }

    if (props.mockMode) {
      // 模拟模式下的延迟
      await new Promise(resolve => setTimeout(resolve, 1000))
      message.success('增强请求发送成功')
    }

    // 通过事件触发统一的响应处理（send_mcp_response + exit_app）
    // 注意：不再在这里直接调用 invoke，避免重复发送
    emit('response', response)
  }
  catch (error) {
    console.error('发送增强请求失败:', error)
    message.error('增强请求失败，请重试')
  }
  finally {
    submitting.value = false
  }
}
</script>

<template>
  <div v-if="isVisible" class="flex flex-col flex-1 min-h-0 overflow-hidden">
    <!-- 内容区域 - 可滚动 -->
    <div class="flex-1 overflow-y-auto scrollbar-thin min-h-0">
      <!-- 消息内容 - 允许选中 -->
      <div class="mx-2 mt-2 mb-1 px-4 py-3 bg-black-100 rounded-lg select-text" data-guide="popup-content">
        <PopupContent :request="request" :loading="loading" :current-theme="props.appConfig.theme" @quote-message="handleQuoteMessage" />
      </div>

      <!-- 输入和选项 - 允许选中 -->
      <div class="px-4 pb-3 bg-black select-text">
        <PopupInput
          ref="inputRef" :request="request" :loading="loading" :submitting="submitting"
          @update="handleInputUpdate" @image-add="handleImageAdd" @image-remove="handleImageRemove"
        />
      </div>
    </div>

    <!-- 底部操作栏 - 固定在底部 -->
    <div class="flex-shrink-0 bg-black-100 border-t-2 border-black-200" data-guide="popup-actions">
      <PopupActions
        :request="request" :loading="loading" :submitting="submitting" :can-submit="canSubmit"
        :continue-reply-enabled="continueReplyEnabled" :input-status-text="inputStatusText"
        :countdown-remaining="countdownRemaining"
        @submit="handleSubmit" @continue="handleContinue" @enhance="handleEnhance"
        @cancel-countdown="cancelCountdown"
      />
    </div>
  </div>
</template>
