import { defineStore } from 'pinia'
import { ref } from 'vue'
import { useApi } from '../composables/useApi'
import type { CronJob, CronRun } from '../types/api'

export const useCronStore = defineStore('cron', () => {
  const api = useApi()
  const jobs = ref<CronJob[]>([])
  const loading = ref(false)

  async function fetchJobs(enabled?: boolean) {
    loading.value = true
    try {
      const params = enabled !== undefined ? `?enabled=${enabled}` : ''
      const resp = await api.get<{ data: CronJob[] }>(`/api/v1/cron/jobs${params}`)
      jobs.value = resp.data
    } catch { /* ignore */ }
    loading.value = false
  }

  async function createJob(job: { name: string; schedule: string; command: string; config?: any }) {
    return api.post<{ data: CronJob }>('/api/v1/cron/jobs', job)
  }

  async function updateJob(id: string, updates: { schedule?: string; enabled?: boolean }) {
    return api.put<{ data: CronJob }>(`/api/v1/cron/jobs/${id}`, updates)
  }

  async function deleteJob(id: string) {
    await api.del(`/api/v1/cron/jobs/${id}`)
  }

  async function triggerJob(id: string) {
    return api.post<{ data: CronRun }>(`/api/v1/cron/jobs/${id}/trigger`, {})
  }

  async function fetchRuns(jobId: string, limit = 20) {
    return api.get<{ data: CronRun[] }>(`/api/v1/cron/jobs/${jobId}/runs?limit=${limit}`)
  }

  return { jobs, loading, fetchJobs, createJob, updateJob, deleteJob, triggerJob, fetchRuns }
})
