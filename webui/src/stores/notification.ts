import { defineStore } from 'pinia'
import { ref, computed } from 'vue'

export interface Notification {
  id: string
  message: string
  type: 'info' | 'success' | 'warning' | 'error'
  timestamp: number
  read: boolean
}

export const useNotificationStore = defineStore('notification', () => {
  const notifications = ref<Notification[]>([])

  const unreadCount = computed(() =>
    notifications.value.filter(n => !n.read).length
  )

  function add(message: string, type: Notification['type'] = 'info') {
    notifications.value.unshift({
      id: crypto.randomUUID(),
      message,
      type,
      timestamp: Date.now(),
      read: false,
    })
  }

  function markRead(id: string) {
    const n = notifications.value.find(n => n.id === id)
    if (n) n.read = true
  }

  function clear() {
    notifications.value = []
  }

  return { notifications, unreadCount, add, markRead, clear }
})
