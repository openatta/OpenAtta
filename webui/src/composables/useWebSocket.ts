import { ref, onUnmounted } from 'vue'
import { useNotificationStore } from '../stores/notification'
import { useTaskStore } from '../stores/task'

export function useWebSocket() {
  const connected = ref(false)
  let ws: WebSocket | null = null
  let reconnectTimer: ReturnType<typeof setTimeout> | null = null

  function connect() {
    const protocol = location.protocol === 'https:' ? 'wss:' : 'ws:'
    ws = new WebSocket(`${protocol}//${location.host}/api/v1/ws`)

    ws.onopen = () => {
      connected.value = true
    }

    ws.onmessage = (event) => {
      try {
        const data = JSON.parse(event.data)
        const notifications = useNotificationStore()
        const taskStore = useTaskStore()

        const eventType = data.event_type || ''

        // Update task store on task-related events
        if (eventType.startsWith('task.')) {
          taskStore.fetchTasks()
          if (taskStore.currentTask && data.entity?.id === taskStore.currentTask.id) {
            taskStore.fetchTask(taskStore.currentTask.id)
          }
        }

        // Show notification for important events
        if (eventType === 'task.completed') {
          notifications.add(`Task ${data.entity?.id?.slice(0, 8)} completed`, 'success')
        } else if (eventType === 'task.failed') {
          notifications.add(`Task ${data.entity?.id?.slice(0, 8)} failed`, 'error')
        } else if (eventType === 'task.waiting_approval') {
          notifications.add(`Task ${data.entity?.id?.slice(0, 8)} needs approval`, 'warning')
        } else {
          notifications.add(`Event: ${eventType || 'unknown'}`, 'info')
        }
      } catch {
        // ignore parse errors
      }
    }

    ws.onclose = () => {
      connected.value = false
      // Auto-reconnect after 3 seconds
      reconnectTimer = setTimeout(connect, 3000)
    }

    ws.onerror = () => {
      ws?.close()
    }
  }

  function disconnect() {
    if (reconnectTimer) {
      clearTimeout(reconnectTimer)
      reconnectTimer = null
    }
    ws?.close()
    ws = null
  }

  onUnmounted(() => disconnect())

  return { connected, connect, disconnect }
}
