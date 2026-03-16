import { defineStore } from 'pinia'
import { ref } from 'vue'
import { useApi } from '../composables/useApi'
import type { MemoryEntry } from '../types/api'

export const useMemoryStore = defineStore('memory', () => {
  const api = useApi()
  const entries = ref<MemoryEntry[]>([])
  const loading = ref(false)

  async function search(query: string) {
    loading.value = true
    try {
      const resp = await api.get<{ data: MemoryEntry[] }>(`/api/v1/memory/search?q=${encodeURIComponent(query)}`)
      entries.value = resp.data
    } catch { /* ignore */ }
    loading.value = false
  }

  async function remove(id: string) {
    await api.del(`/api/v1/memory/${id}`)
    entries.value = entries.value.filter(e => e.id !== id)
  }

  return { entries, loading, search, remove }
})
