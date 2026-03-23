import { computed, ref } from 'vue'
import { useAudioManager } from './useAudioManager'
import { useFontManager } from './useFontManager'
import { usePopupSession } from './usePopupSession'
import { useSettings } from './useSettings'
import { useTheme } from './useTheme'

export function usePopupAppManager() {
  const theme = useTheme()
  const settings = useSettings()
  const audioManager = useAudioManager()
  const popupSession = usePopupSession()
  const { loadFontConfig, loadFontOptions } = useFontManager()
  const isInitializing = ref(true)

  const appConfig = computed(() => {
    return {
      theme: theme.currentTheme.value,
      window: {
        alwaysOnTop: settings.alwaysOnTop.value,
        width: settings.windowWidth.value,
        height: settings.windowHeight.value,
        fixed: settings.fixedWindowSize.value,
      },
      audio: {
        enabled: settings.audioNotificationEnabled.value,
        url: settings.audioUrl.value,
      },
      reply: {
        enabled: settings.continueReplyEnabled.value,
        prompt: settings.continuePrompt.value,
      },
    }
  })

  async function initializePopupApp() {
    try {
      await Promise.all([
        loadFontConfig(),
        loadFontOptions(),
      ])

      await settings.loadWindowSettings()
      await settings.loadWindowConfig()
      await settings.setupWindowFocusListener()

      try {
        await settings.syncWindowStateFromBackend()
      }
      catch (error) {
        console.warn('popup 窗口状态同步失败，继续初始化:', error)
      }

      await popupSession.loadCurrentRequest()
      isInitializing.value = false
    }
    catch (error) {
      isInitializing.value = false
      throw error
    }
  }

  const actions = {
    theme: {
      setTheme: theme.setTheme,
    },
    settings: {
      toggleAlwaysOnTop: settings.toggleAlwaysOnTop,
      toggleAudioNotification: settings.toggleAudioNotification,
      updateAudioUrl: settings.updateAudioUrl,
      testAudio: settings.testAudioSound,
      stopAudio: settings.stopAudioSound,
      updateWindowSize: settings.updateWindowSize,
      updateReplyConfig: settings.updateReplyConfig,
      setMessageInstance: settings.setMessageInstance,
      reloadAllSettings: settings.reloadAllSettings,
    },
    mcp: {
      handleResponse: popupSession.handleMcpResponse,
      handleCancel: popupSession.handleMcpCancel,
    },
    audio: {
      handleTestError: audioManager.handleTestAudioError,
    },
    app: {
      initialize: initializePopupApp,
      cleanup: () => {
        settings.removeWindowFocusListener()
      },
    },
  }

  return {
    naiveTheme: theme.naiveTheme,
    mcpRequest: popupSession.mcpRequest,
    pendingCount: computed(() => 0),
    showMcpPopup: popupSession.showMcpPopup,
    appConfig,
    isInitializing,
    actions,
  }
}
