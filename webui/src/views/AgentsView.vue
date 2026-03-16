<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { useApi } from '../composables/useApi'
import { useI18n } from 'vue-i18n'
import Pagination from '../components/common/Pagination.vue'

const { t } = useI18n()

interface Agent {
  id: string
  task: string
  status: string
  created_at: string
  elapsed_ms: number
}

const api = useApi()
const agents = ref<Agent[]>([])
const loading = ref(false)
const currentPage = ref(1)
const pageSize = ref(20)

const paginatedAgents = computed(() => {
  const start = (currentPage.value - 1) * pageSize.value
  return agents.value.slice(start, start + pageSize.value)
})

function translateStatus(status: string): string {
  const key = `common.status_${status}`
  const translated = t(key)
  return translated !== key ? translated : status
}

async function fetchAgents() {
  loading.value = true
  try {
    agents.value = await api.get<Agent[]>('/api/v1/agents').catch(() => [])
    currentPage.value = 1
  } finally {
    loading.value = false
  }
}

async function terminateAgent(id: string) {
  try {
    await api.post(`/api/v1/agents/${id}/terminate`, {})
    await fetchAgents()
  } catch {
    // Error toast shown by global onResponseError handler
  }
}

async function pauseAgent(id: string) {
  try {
    await api.post(`/api/v1/agents/${id}/pause`, {})
    await fetchAgents()
  } catch {
    // Error toast shown by global onResponseError handler
  }
}

async function resumeAgent(id: string) {
  try {
    await api.post(`/api/v1/agents/${id}/resume`, {})
    await fetchAgents()
  } catch {
    // Error toast shown by global onResponseError handler
  }
}

function formatDuration(ms: number): string {
  if (ms < 1000) return `${ms}ms`
  if (ms < 60000) return `${(ms / 1000).toFixed(1)}s`
  return `${(ms / 60000).toFixed(1)}m`
}

onMounted(fetchAgents)
</script>

<template>
  <div class="agents-view">
    <div class="page-header">
      <h2>{{ t('agents.title') }}</h2>
      <button class="btn btn-secondary" @click="fetchAgents">{{ t('common.refresh') }}</button>
    </div>

    <div class="card">
      <table v-if="agents.length">
        <thead>
          <tr>
            <th>{{ t('agents.id') }}</th>
            <th>{{ t('agents.task') }}</th>
            <th>{{ t('agents.status') }}</th>
            <th>{{ t('agents.elapsed') }}</th>
            <th>{{ t('agents.actions') }}</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="agent in paginatedAgents" :key="agent.id">
            <td class="mono">{{ agent.id.slice(0, 8) }}</td>
            <td>{{ agent.task }}</td>
            <td>
              <span :class="['status-badge', `status-${agent.status}`]">
                {{ translateStatus(agent.status) }}
              </span>
            </td>
            <td>{{ formatDuration(agent.elapsed_ms) }}</td>
            <td class="actions">
              <button
                v-if="agent.status === 'running'"
                class="btn btn-sm btn-secondary"
                @click="pauseAgent(agent.id)"
              >{{ t('agents.pause') }}</button>
              <button
                v-if="agent.status === 'paused'"
                class="btn btn-sm btn-primary"
                @click="resumeAgent(agent.id)"
              >{{ t('agents.resume') }}</button>
              <button
                v-if="agent.status === 'running' || agent.status === 'paused'"
                class="btn btn-sm btn-danger"
                @click="terminateAgent(agent.id)"
              >{{ t('agents.terminate') }}</button>
            </td>
          </tr>
        </tbody>
      </table>
      <p v-else class="empty">{{ t('agents.no_agents') }}</p>
      <Pagination
        v-if="agents.length"
        :total="agents.length"
        :page-size="pageSize"
        v-model:current-page="currentPage"
      />
    </div>
  </div>
</template>

<style scoped>
.page-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 1.25rem;
}
table {
  width: 100%;
  border-collapse: collapse;
}
th, td {
  text-align: left;
  padding: 0.5rem 0.75rem;
  border-bottom: 1px solid var(--border-color);
}
th {
  font-weight: 600;
  font-size: 0.8125rem;
  color: var(--text-secondary);
}
.mono { font-family: monospace; }
.actions {
  display: flex;
  gap: 0.25rem;
}
.btn-sm {
  padding: 0.25rem 0.5rem;
  font-size: 0.75rem;
}
.btn-secondary {
  background: #e5e7eb;
  color: var(--text-primary);
}
.btn-danger {
  background: var(--color-error);
  color: white;
}
.status-paused {
  background: #fef3c7;
  color: #92400e;
}
.status-terminated {
  background: #f3f4f6;
  color: #6b7280;
}
.empty {
  color: var(--text-secondary);
  padding: 2rem 0;
  text-align: center;
}
</style>
