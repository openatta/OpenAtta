import { defineStore } from 'pinia'
import { ref } from 'vue'
import { useApi } from '../composables/useApi'
import type { Task, TaskFilter } from '../types/api'

export const useTaskStore = defineStore('task', () => {
  const tasks = ref<Task[]>([])
  const currentTask = ref<Task | null>(null)
  const loading = ref(false)
  const api = useApi()

  async function fetchTasks(filter?: TaskFilter) {
    loading.value = true
    try {
      const params = new URLSearchParams()
      if (filter?.status) params.set('status', filter.status)
      if (filter?.flow_id) params.set('flow_id', filter.flow_id)
      const url = params.toString() ? `/api/v1/tasks?${params}` : '/api/v1/tasks'
      tasks.value = await api.get<Task[]>(url)
    } finally {
      loading.value = false
    }
  }

  async function fetchTask(id: string) {
    loading.value = true
    try {
      currentTask.value = await api.get<Task>(`/api/v1/tasks/${id}`)
    } finally {
      loading.value = false
    }
  }

  async function createTask(flowId: string, input: Record<string, unknown>) {
    return api.post<Task>('/api/v1/tasks', { flow_id: flowId, input })
  }

  async function cancelTask(id: string) {
    return api.post(`/api/v1/tasks/${id}/cancel`, {})
  }

  return { tasks, currentTask, loading, fetchTasks, fetchTask, createTask, cancelTask }
})
