import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import type { LogEntry } from '../types/api'

export const useLogStore = defineStore('log', () => {
  const entries = ref<LogEntry[]>([])
  const connected = ref(false)
  const paused = ref(false)
  const maxEntries = 500
  const levelFilter = ref<Set<string>>(new Set(['trace', 'debug', 'info', 'warn', 'error']))
  const textFilter = ref('')

  let eventSource: EventSource | null = null

  const filtered = computed(() => {
    return entries.value.filter(e => {
      if (!levelFilter.value.has(e.level)) return false
      if (textFilter.value) {
        const q = textFilter.value.toLowerCase()
        return e.message.toLowerCase().includes(q) || e.target.toLowerCase().includes(q)
      }
      return true
    })
  })

  async function fetchRecent() {
    try {
      const resp = await fetch('/api/v1/logs/recent')
      if (!resp.ok) return
      const data: LogEntry[] = await resp.json()
      if (data.length) {
        entries.value = data.slice(-maxEntries)
      }
    } catch { /* ignore */ }
  }

  async function connect() {
    if (eventSource) return
    await fetchRecent()
    eventSource = new EventSource('/api/v1/logs/stream')

    eventSource.onopen = () => {
      connected.value = true
    }

    eventSource.onmessage = (event) => {
      if (paused.value) return
      try {
        const entry: LogEntry = JSON.parse(event.data)
        entries.value.push(entry)
        if (entries.value.length > maxEntries) {
          entries.value.splice(0, entries.value.length - maxEntries)
        }
      } catch { /* ignore */ }
    }

    eventSource.onerror = () => {
      connected.value = false
      disconnect()
      // Auto-reconnect after 3s
      setTimeout(connect, 3000)
    }
  }

  function disconnect() {
    eventSource?.close()
    eventSource = null
    connected.value = false
  }

  function clear() {
    entries.value = []
  }

  function toggleLevel(level: string) {
    if (levelFilter.value.has(level)) {
      levelFilter.value.delete(level)
    } else {
      levelFilter.value.add(level)
    }
    // Trigger reactivity
    levelFilter.value = new Set(levelFilter.value)
  }

  return {
    entries, connected, paused, levelFilter, textFilter,
    filtered, connect, disconnect, clear, toggleLevel
  }
})
