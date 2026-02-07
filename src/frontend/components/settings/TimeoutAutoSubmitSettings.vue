<script setup lang="ts">
import { invoke } from '@tauri-apps/api/core'
import { onMounted, ref, watch } from 'vue'

let saveTimer: ReturnType<typeof setTimeout> | null = null

interface TimeoutAutoSubmitConfig {
  enabled: boolean
  timeout_seconds: number
  prompt_source: string // "continue" | "custom" | "manual"
  custom_prompt_id: string | null
  manual_prompt: string
}

interface CustomPrompt {
  id: string
  name: string
  content: string
  type: string
}

const localConfig = ref<TimeoutAutoSubmitConfig>({
  enabled: false,
  timeout_seconds: 360,
  prompt_source: 'continue',
  custom_prompt_id: null,
  manual_prompt: '请按照最佳实践继续',
})

const normalPrompts = ref<CustomPrompt[]>([])

// 加载配置
async function loadConfig() {
  try {
    const config = await invoke('get_timeout_auto_submit_config')
    localConfig.value = config as TimeoutAutoSubmitConfig
  }
  catch (error) {
    console.error('加载超时自动提交配置失败:', error)
  }
}

// 加载自定义prompt列表（仅 normal 类型）
async function loadCustomPrompts() {
  try {
    const config = await invoke('get_custom_prompt_config') as any
    if (config?.prompts) {
      normalPrompts.value = config.prompts.filter((p: CustomPrompt) => p.type === 'normal')
    }
  }
  catch (error) {
    console.error('加载自定义prompt列表失败:', error)
  }
}

// 更新配置
async function updateConfig() {
  try {
    await invoke('set_timeout_auto_submit_config', { timeoutAutoSubmitConfig: localConfig.value })
  }
  catch (error) {
    console.error('保存超时自动提交配置失败:', error)
  }
}

// 防抖保存（用于文本输入场景）
function debouncedUpdateConfig() {
  if (saveTimer)
    clearTimeout(saveTimer)
  saveTimer = setTimeout(() => updateConfig(), 500)
}

// 当切换到 custom 来源时加载 prompt 列表
watch(() => localConfig.value.prompt_source, (newVal) => {
  if (newVal === 'custom') {
    loadCustomPrompts()
  }
})

// prompt 选项列表
const promptOptions = ref<Array<{ label: string, value: string }>>([])
watch(normalPrompts, (prompts) => {
  promptOptions.value = prompts.map(p => ({
    label: p.name,
    value: p.id,
  }))
}, { immediate: true })

onMounted(() => {
  loadConfig()
  loadCustomPrompts()
})
</script>

<template>
  <n-space vertical size="large">
    <!-- 启用开关 -->
    <div class="flex items-center justify-between">
      <div class="flex items-center">
        <div class="w-1.5 h-1.5 bg-warning rounded-full mr-3 flex-shrink-0" />
        <div>
          <div class="text-sm font-medium leading-relaxed">
            启用超时自动提交
          </div>
          <div class="text-xs opacity-60">
            弹窗超时后自动发送配置的提示词
          </div>
        </div>
      </div>
      <n-switch
        v-model:value="localConfig.enabled"
        size="small"
        @update:value="updateConfig"
      />
    </div>

    <template v-if="localConfig.enabled">
      <!-- 等待时间 -->
      <div>
        <div class="flex items-center mb-3">
          <div class="w-1.5 h-1.5 bg-warning rounded-full mr-3 flex-shrink-0" />
          <div>
            <div class="text-sm font-medium leading-relaxed">
              等待时间（秒）
            </div>
            <div class="text-xs opacity-60">
              弹窗出现后等待多少秒自动提交（5-3600）
            </div>
          </div>
        </div>
        <n-input-number
          v-model:value="localConfig.timeout_seconds"
          :min="5"
          :max="3600"
          :step="1"
          size="small"
          @update:value="updateConfig"
        />
      </div>

      <!-- 提示词来源 -->
      <div>
        <div class="flex items-center mb-3">
          <div class="w-1.5 h-1.5 bg-warning rounded-full mr-3 flex-shrink-0" />
          <div>
            <div class="text-sm font-medium leading-relaxed">
              提示词来源
            </div>
            <div class="text-xs opacity-60">
              选择超时后自动发送的提示词来源
            </div>
          </div>
        </div>
        <n-radio-group
          v-model:value="localConfig.prompt_source"
          @update:value="updateConfig"
        >
          <n-space vertical>
            <n-radio value="continue">
              使用继续提示词
            </n-radio>
            <n-radio value="custom">
              从模板选择
            </n-radio>
            <n-radio value="manual">
              手动输入
            </n-radio>
          </n-space>
        </n-radio-group>
      </div>

      <!-- 模板选择 -->
      <div v-if="localConfig.prompt_source === 'custom'">
        <div class="flex items-center mb-3">
          <div class="w-1.5 h-1.5 bg-warning rounded-full mr-3 flex-shrink-0" />
          <div>
            <div class="text-sm font-medium leading-relaxed">
              选择模板
            </div>
            <div class="text-xs opacity-60">
              从提示词模板中选择（仅普通类型）
            </div>
          </div>
        </div>
        <n-select
          v-model:value="localConfig.custom_prompt_id"
          :options="promptOptions"
          size="small"
          placeholder="请选择模板"
          clearable
          @update:value="updateConfig"
        />
      </div>

      <!-- 手动输入 -->
      <div v-if="localConfig.prompt_source === 'manual'">
        <div class="flex items-center mb-3">
          <div class="w-1.5 h-1.5 bg-warning rounded-full mr-3 flex-shrink-0" />
          <div>
            <div class="text-sm font-medium leading-relaxed">
              手动提示词
            </div>
            <div class="text-xs opacity-60">
              超时后自动发送的提示词内容
            </div>
          </div>
        </div>
        <n-input
          v-model:value="localConfig.manual_prompt"
          type="textarea"
          size="small"
          placeholder="请输入超时后自动发送的提示词"
          :autosize="{ minRows: 2, maxRows: 5 }"
          @input="debouncedUpdateConfig"
        />
      </div>
    </template>
  </n-space>
</template>
