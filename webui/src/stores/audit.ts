import { defineStore } from 'pinia'
import { ref } from 'vue'
import { useApi } from '../composables/useApi'
import type { AuditEntry } from '../types/api'

export const useAuditStore = defineStore('audit', () => {
  const api = useApi()
  const entries = ref<AuditEntry[]>([])
  const loading = ref(false)
  const total = ref(0)

  async function fetchEntries(params: { actor_type?: string; action?: string; limit?: number; offset?: number } = {}) {
    loading.value = true
    try {
      const query = new URLSearchParams()
      if (params.actor_type) query.set('actor_type', params.actor_type)
      if (params.action) query.set('action', params.action)
      query.set('limit', String(params.limit || 50))
      query.set('offset', String(params.offset || 0))
      const resp = await api.get<{ data: AuditEntry[]; total: number }>(`/api/v1/audit?${query}`)
      entries.value = resp.data
      total.value = resp.total
    } catch { /* handled by global error */ }
    loading.value = false
  }

  async function exportCsv(start?: string, end?: string) {
    const query = new URLSearchParams()
    if (start) query.set('start', start)
    if (end) query.set('end', end)
    query.set('format', 'csv')
    const url = `/api/v1/audit/export?${query}`
    window.open(url, '_blank')
  }

  return { entries, loading, total, fetchEntries, exportCsv }
})
