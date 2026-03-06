<script setup lang="ts">
import { invoke } from '@tauri-apps/api/core'
import { onMounted, ref } from 'vue'

interface ConversationRecord {
  id: string
  request_message: string
  request_options: string[]
  response_text: string
  selected_options: string[]
  timestamp: string
  source: string
}

const records = ref<ConversationRecord[]>([])
const loading = ref(false)
const expandedId = ref<string | null>(null)

async function loadHistory() {
  loading.value = true
  try {
    records.value = (await invoke('get_conversation_history') as ConversationRecord[]) || []
  }
  catch {
    records.value = []
  }
  finally {
    loading.value = false
  }
}

function toggleExpand(id: string) {
  expandedId.value = expandedId.value === id ? null : id
}

function truncate(text: string, max: number): string {
  if (!text)
    return ''
  return text.length > max ? `${text.substring(0, max)}...` : text
}

function sourceLabel(source: string): string {
  const map: Record<string, string> = {
    popup: '手动提交',
    popup_continue: '继续',
    popup_enhance: '增强',
    popup_timeout_auto_submit: '超时自动',
  }
  return map[source] || source
}

function formatTime(ts: string): string {
  if (!ts)
    return ''
  try {
    const d = new Date(ts)
    return `${d.toLocaleDateString()} ${d.toLocaleTimeString()}`
  }
  catch {
    return ts
  }
}

onMounted(() => {
  loadHistory()
})

defineExpose({ loadHistory })
</script>

<template>
  <div class="flex-1 overflow-y-auto scrollbar-thin p-4">
    <div class="flex items-center justify-between mb-4">
      <h2 class="text-base font-medium text-white">
        最近对话历史
      </h2>
      <n-button size="small" quaternary @click="loadHistory">
        <template #icon>
          <div class="i-carbon-renew w-4 h-4" />
        </template>
        刷新
      </n-button>
    </div>

    <!-- Loading -->
    <div v-if="loading" class="flex justify-center py-12">
      <n-spin size="medium" />
    </div>

    <!-- Empty -->
    <div v-else-if="records.length === 0" class="flex flex-col items-center justify-center py-12 text-gray-500">
      <div class="i-carbon-chat-off w-12 h-12 mb-3 opacity-40" />
      <div class="text-sm">
        暂无对话记录
      </div>
    </div>

    <!-- Records -->
    <div v-else class="space-y-3">
      <div
        v-for="record in [...records].reverse()"
        :key="record.id"
        class="bg-black-100 border border-gray-700 rounded-lg p-3 cursor-pointer hover:border-gray-500 transition-colors"
        @click="toggleExpand(record.id)"
      >
        <!-- Header -->
        <div class="flex items-center justify-between mb-2">
          <span class="text-xs text-gray-500 font-mono">{{ formatTime(record.timestamp) }}</span>
          <span class="text-xs px-1.5 py-0.5 rounded bg-gray-800 text-gray-400">{{ sourceLabel(record.source) }}</span>
        </div>

        <!-- Request (AI message) -->
        <div class="text-sm text-gray-300 mb-1.5">
          <span class="text-gray-600 text-xs mr-1">AI:</span>
          <span v-if="expandedId === record.id" class="whitespace-pre-wrap">{{ record.request_message }}</span>
          <span v-else>{{ truncate(record.request_message, 100) }}</span>
        </div>

        <!-- Options (if any) -->
        <div v-if="record.selected_options.length > 0" class="flex flex-wrap gap-1 mb-1.5">
          <span
            v-for="opt in record.selected_options"
            :key="opt"
            class="text-xs px-1.5 py-0.5 rounded bg-primary-500/20 text-primary-400"
          >{{ opt }}</span>
        </div>

        <!-- Response -->
        <div class="text-sm text-green-400/80">
          <span class="text-gray-600 text-xs mr-1">回复:</span>
          <span v-if="expandedId === record.id" class="whitespace-pre-wrap">{{ record.response_text || '(无文字输入)' }}</span>
          <span v-else>{{ truncate(record.response_text, 80) || '(无文字输入)' }}</span>
        </div>

        <!-- Expand hint -->
        <div class="text-center mt-1">
          <div
            :class="expandedId === record.id ? 'i-carbon-chevron-up' : 'i-carbon-chevron-down'"
            class="w-3 h-3 text-gray-600 inline-block"
          />
        </div>
      </div>
    </div>
  </div>
</template>
