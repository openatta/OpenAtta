<script setup lang="ts">
import { ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useNotificationStore, type Notification } from '../../stores/notification'

const { t } = useI18n()
const store = useNotificationStore()

interface Toast {
  id: string
  message: string
  type: Notification['type']
  timer: ReturnType<typeof setTimeout>
}

const toasts = ref<Toast[]>([])
const MAX_TOASTS = 5

function dismiss(id: string) {
  const idx = toasts.value.findIndex(t => t.id === id)
  if (idx !== -1) {
    clearTimeout(toasts.value[idx].timer)
    toasts.value.splice(idx, 1)
  }
}

watch(
  () => store.notifications.length,
  (newLen, oldLen) => {
    if (newLen <= oldLen) return
    const notification = store.notifications[0]
    if (!notification) return
    // Avoid duplicate toasts for the same notification
    if (toasts.value.some(t => t.id === notification.id)) return

    const timer = setTimeout(() => dismiss(notification.id), 5000)
    toasts.value.push({
      id: notification.id,
      message: notification.message,
      type: notification.type,
      timer,
    })

    // Limit visible toasts
    while (toasts.value.length > MAX_TOASTS) {
      const removed = toasts.value.shift()
      if (removed) clearTimeout(removed.timer)
    }
  },
)
</script>

<template>
  <div class="toast-container" aria-live="polite">
    <TransitionGroup name="toast">
      <div
        v-for="toast in toasts"
        :key="toast.id"
        class="toast"
        :class="`toast--${toast.type}`"
      >
        <span class="toast-message">{{ toast.message }}</span>
        <button
          class="toast-close"
          :aria-label="t('toast.dismiss')"
          @click="dismiss(toast.id)"
        >
          &times;
        </button>
      </div>
    </TransitionGroup>
  </div>
</template>

<style scoped>
.toast-container {
  position: fixed;
  bottom: 1rem;
  right: 1rem;
  z-index: 9999;
  display: flex;
  flex-direction: column-reverse;
  gap: 0.5rem;
  max-width: 380px;
  pointer-events: none;
}

.toast {
  display: flex;
  align-items: flex-start;
  gap: 0.5rem;
  padding: 0.75rem 1rem;
  border-radius: 6px;
  font-size: 0.8125rem;
  line-height: 1.4;
  color: #fff;
  box-shadow: 0 4px 12px rgba(0, 0, 0, 0.15);
  pointer-events: auto;
}

.toast--error {
  background: #dc3545;
}

.toast--warning {
  background: #e6a700;
  color: #1a1a1a;
}

.toast--success {
  background: #28a745;
}

.toast--info {
  background: #0d6efd;
}

.toast-message {
  flex: 1;
  word-break: break-word;
}

.toast-close {
  background: none;
  border: none;
  color: inherit;
  font-size: 1.125rem;
  line-height: 1;
  cursor: pointer;
  padding: 0;
  opacity: 0.8;
  flex-shrink: 0;
}

.toast-close:hover {
  opacity: 1;
}

/* Transitions */
.toast-enter-active {
  transition: all 0.3s ease;
}

.toast-leave-active {
  transition: all 0.25s ease;
}

.toast-enter-from {
  opacity: 0;
  transform: translateX(80px);
}

.toast-leave-to {
  opacity: 0;
  transform: translateX(80px);
}
</style>
