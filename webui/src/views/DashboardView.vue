<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useTaskStore } from '../stores/task'
import { useWebSocket } from '../composables/useWebSocket'
import { useI18n } from 'vue-i18n'
import Pagination from '../components/common/Pagination.vue'

const { t } = useI18n()
const taskStore = useTaskStore()
const { connected, connect } = useWebSocket()
const currentPage = ref(1)
const pageSize = ref(10)

const paginatedTasks = computed(() => {
  const start = (currentPage.value - 1) * pageSize.value
  return taskStore.tasks.slice(start, start + pageSize.value)
})

function translateStatus(status: string): string {
  const key = `common.status_${status}`
  const translated = t(key)
  return translated !== key ? translated : status
}

onMounted(() => {
  taskStore.fetchTasks()
  connect()
})
</script>

<template>
  <div class="dashboard">
    <h2>{{ t('dashboard.title') }}</h2>
    <div class="stats">
      <div class="card stat-card">
        <div class="stat-value">{{ taskStore.tasks.length }}</div>
        <div class="stat-label">{{ t('dashboard.total_tasks') }}</div>
      </div>
      <div class="card stat-card">
        <div class="stat-value">{{ taskStore.tasks.filter(t => t.status === 'running').length }}</div>
        <div class="stat-label">{{ t('dashboard.running') }}</div>
      </div>
      <div class="card stat-card">
        <div class="stat-value">{{ taskStore.tasks.filter(t => t.status === 'completed').length }}</div>
        <div class="stat-label">{{ t('dashboard.completed') }}</div>
      </div>
      <div class="card stat-card">
        <div class="stat-value" :class="{ 'text-green': connected }">
          {{ connected ? t('common.connected') : t('common.disconnected') }}
        </div>
        <div class="stat-label">{{ t('dashboard.websocket') }}</div>
      </div>
    </div>

    <div class="card recent-tasks">
      <h3>{{ t('dashboard.recent_tasks') }}</h3>
      <table v-if="taskStore.tasks.length">
        <thead>
          <tr>
            <th>{{ t('dashboard.id') }}</th>
            <th>{{ t('dashboard.flow') }}</th>
            <th>{{ t('dashboard.state') }}</th>
            <th>{{ t('common.status') }}</th>
            <th>{{ t('dashboard.created') }}</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="task in paginatedTasks" :key="task.id">
            <td class="mono">{{ task.id.slice(0, 8) }}</td>
            <td>{{ task.flow_id }}</td>
            <td>{{ task.current_state }}</td>
            <td>
              <span :class="['status-badge', `status-${typeof task.status === 'string' ? task.status : 'failed'}`]">
                {{ translateStatus(typeof task.status === 'string' ? task.status : 'failed') }}
              </span>
            </td>
            <td>{{ new Date(task.created_at).toLocaleString() }}</td>
          </tr>
        </tbody>
      </table>
      <Pagination
        v-if="taskStore.tasks.length"
        :total="taskStore.tasks.length"
        :page-size="pageSize"
        v-model:current-page="currentPage"
      />
      <p v-else class="empty">{{ t('dashboard.no_tasks') }}</p>
    </div>
  </div>
</template>

<style scoped>
.dashboard h2 {
  margin-bottom: 1.25rem;
}
.stats {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
  gap: 1rem;
  margin-bottom: 1.5rem;
}
.stat-card {
  text-align: center;
}
.stat-value {
  font-size: 1.75rem;
  font-weight: 700;
}
.stat-label {
  color: var(--text-secondary);
  font-size: 0.875rem;
}
.text-green {
  color: var(--color-success);
}
.recent-tasks h3 {
  margin-bottom: 1rem;
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
.mono {
  font-family: monospace;
}
.empty {
  color: var(--text-secondary);
  padding: 2rem 0;
  text-align: center;
}
</style>
