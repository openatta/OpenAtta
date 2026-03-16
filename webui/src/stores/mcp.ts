import { defineStore } from 'pinia'
import { ref } from 'vue'
import { useApi } from '../composables/useApi'
import type { McpServerConfig, McpServerInfo } from '../types/api'

export const useMcpStore = defineStore('mcp', () => {
  const servers = ref<string[]>([])
  const loading = ref(false)
  const api = useApi()

  async function fetchMcpServers() {
    loading.value = true
    try {
      const res = await api.get<{ servers: string[] }>('/api/v1/mcp/servers')
      servers.value = res.servers
    } finally {
      loading.value = false
    }
  }

  async function getMcpServer(name: string) {
    return api.get<McpServerInfo>(`/api/v1/mcp/servers/${name}`)
  }

  async function registerMcp(config: McpServerConfig) {
    return api.post('/api/v1/mcp/servers', config as unknown as Record<string, any>)
  }

  async function unregisterMcp(name: string) {
    return api.del(`/api/v1/mcp/servers/${name}`)
  }

  return { servers, loading, fetchMcpServers, getMcpServer, registerMcp, unregisterMcp }
})
