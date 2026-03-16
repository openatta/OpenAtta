import { defineStore } from 'pinia'
import { ref } from 'vue'
import { useApi } from '../composables/useApi'
import type { FlowDef } from '../types/api'

export const useFlowStore = defineStore('flow', () => {
  const flows = ref<FlowDef[]>([])
  const loading = ref(false)
  const api = useApi()

  async function fetchFlows() {
    loading.value = true
    try {
      flows.value = await api.get<FlowDef[]>('/api/v1/flows')
    } finally {
      loading.value = false
    }
  }

  async function createFlow(flow: FlowDef) {
    return api.post<FlowDef>('/api/v1/flows', flow)
  }

  async function updateFlow(id: string, flow: FlowDef) {
    return api.put<FlowDef>(`/api/v1/flows/${id}`, flow)
  }

  async function deleteFlow(id: string) {
    return api.del(`/api/v1/flows/${id}`)
  }

  return { flows, loading, fetchFlows, createFlow, updateFlow, deleteFlow }
})
