import { defineStore } from 'pinia'
import { ref } from 'vue'
import { useApi } from '../composables/useApi'
import type { UsageSummary, UsageDaily, ModelUsage } from '../types/api'

export const useUsageStore = defineStore('usage', () => {
  const api = useApi()
  const summary = ref<UsageSummary | null>(null)
  const daily = ref<UsageDaily[]>([])
  const byModel = ref<ModelUsage[]>([])
  const loading = ref(false)

  async function fetchSummary(period = '30d') {
    loading.value = true
    try {
      const resp = await api.get<{ data: UsageSummary }>(`/api/v1/usage/summary?period=${period}`)
      summary.value = resp.data
    } catch { /* ignore */ }
    loading.value = false
  }

  async function fetchDaily(start: string, end: string) {
    try {
      const resp = await api.get<{ data: UsageDaily[] }>(`/api/v1/usage/daily?start=${start}&end=${end}`)
      daily.value = resp.data
    } catch { /* ignore */ }
  }

  async function fetchByModel(period = '30d') {
    try {
      const resp = await api.get<{ data: ModelUsage[] }>(`/api/v1/usage/by-model?period=${period}`)
      byModel.value = resp.data
    } catch { /* ignore */ }
  }

  async function exportCsv(period = '30d') {
    try {
      const resp = await fetch(`/api/v1/usage/export?period=${period}`)
      if (!resp.ok) return
      const blob = await resp.blob()
      const url = URL.createObjectURL(blob)
      const a = document.createElement('a')
      a.href = url
      a.download = `attaos-usage-${period}.csv`
      a.click()
      URL.revokeObjectURL(url)
    } catch { /* ignore */ }
  }

  return { summary, daily, byModel, loading, fetchSummary, fetchDaily, fetchByModel, exportCsv }
})
