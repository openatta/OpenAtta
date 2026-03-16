<script setup lang="ts">
import { useNotificationStore } from '../../stores/notification'
import { useThemeStore } from '../../stores/theme'
import { useI18n } from 'vue-i18n'
import { currentLocale, setLocale } from '../../i18n'

const notifications = useNotificationStore()
const theme = useThemeStore()
const { t } = useI18n()

function toggleLang() {
  const next = currentLocale.value === 'en' ? 'zh-CN' : 'en'
  setLocale(next)
}
</script>

<template>
  <header class="topbar">
    <div class="topbar-title">
      <slot />
    </div>
    <div class="topbar-actions">
      <button class="btn-icon" @click="toggleLang" :title="t('common.switch_language')">
        {{ currentLocale === 'en' ? '中' : 'EN' }}
      </button>
      <button class="btn-icon" @click="theme.toggle()" :title="theme.mode === 'light' ? t('common.dark_mode') : t('common.light_mode')">
        {{ theme.mode === 'light' ? '🌙' : '☀️' }}
      </button>
      <span v-if="notifications.unreadCount > 0" class="badge">
        {{ notifications.unreadCount }}
      </span>
    </div>
  </header>
</template>

<style scoped>
.topbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0.75rem 1.5rem;
  border-bottom: 1px solid var(--border-color);
  background: var(--bg-surface);
}
.topbar-actions {
  display: flex;
  align-items: center;
  gap: 0.75rem;
}
.badge {
  background: var(--color-primary);
  color: white;
  font-size: 0.75rem;
  padding: 0.125rem 0.5rem;
  border-radius: 999px;
}
</style>
