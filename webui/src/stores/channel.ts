import { defineStore } from 'pinia'
import { ref } from 'vue'
import { useApi } from '../composables/useApi'
import type { ChannelInfo } from '../types/api'

export const useChannelStore = defineStore('channel', () => {
  const api = useApi()
  const channels = ref<ChannelInfo[]>([])
  const loading = ref(false)

  async function fetchChannels() {
    loading.value = true
    try {
      const resp = await api.get<{ data: ChannelInfo[] }>('/api/v1/channels')
      channels.value = resp.data
    } catch { /* ignore */ }
    loading.value = false
  }

  async function checkHealth(name: string) {
    return api.get<{ data: any }>(`/api/v1/channels/${name}/health`)
  }

  async function addChannel(config: any) {
    return api.post<{ data: any }>('/api/v1/channels', config)
  }

  async function removeChannel(name: string) {
    await api.del(`/api/v1/channels/${name}`)
  }

  async function updateChannel(name: string, config: any) {
    return api.put<{ data: any }>(`/api/v1/channels/${name}`, config)
  }

  return { channels, loading, fetchChannels, checkHealth, addChannel, removeChannel, updateChannel }
})
