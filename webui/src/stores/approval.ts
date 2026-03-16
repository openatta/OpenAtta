import { defineStore } from 'pinia'
import { ref } from 'vue'
import { useApi } from '../composables/useApi'
import type { Approval } from '../types/api'

export const useApprovalStore = defineStore('approval', () => {
  const api = useApi()
  const approvals = ref<Approval[]>([])
  const loading = ref(false)

  async function fetchApprovals(status?: string) {
    loading.value = true
    try {
      const params = status ? `?status=${status}` : ''
      const resp = await api.get<{ data: Approval[] }>(`/api/v1/approvals${params}`)
      approvals.value = resp.data
    } catch { /* handled by global error */ }
    loading.value = false
  }

  async function approve(id: string, comment?: string) {
    await api.post(`/api/v1/approvals/${id}/approve`, { comment })
    await fetchApprovals()
  }

  async function deny(id: string, comment?: string) {
    await api.post(`/api/v1/approvals/${id}/deny`, { comment })
    await fetchApprovals()
  }

  async function requestChanges(id: string, comment?: string) {
    await api.post(`/api/v1/approvals/${id}/request-changes`, { comment })
    await fetchApprovals()
  }

  return { approvals, loading, fetchApprovals, approve, deny, requestChanges }
})
