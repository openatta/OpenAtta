<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import { useTaskStore } from '../stores/task'
import { useI18n } from 'vue-i18n'
import Pagination from '../components/common/Pagination.vue'

const { t } = useI18n()
const taskStore = useTaskStore()
const router = useRouter()
const statusFilter = ref('')
const currentPage = ref(1)
const pageSize = ref(20)

function translateStatus(status: string): string {
  const key = `common.status_${status}`
  const translated = t(key)
  return translated !== key ? translated : status
}

function applyFilter() {
  currentPage.value = 1
  taskStore.fetchTasks(statusFilter.value ? { status: statusFilter.value } : undefined)
}

const paginatedTasks = computed(() => {
  const start = (currentPage.value - 1) * pageSize.value
  return taskStore.tasks.slice(start, start + pageSize.value)
})

onMounted(() => taskStore.fetchTasks())
</script>

<template>
  <div class="tasks-view">
    <div class="page-header">
      <h2>{{ t('tasks.title') }}</h2>
      <div class="header-actions">
        <select v-model="statusFilter" class="filter-select" @change="applyFilter">
          <option value="">{{ t('tasks.all_status') }}</option>
          <option value="running">{{ t('common.running') }}</option>
          <option value="waiting_approval">{{ t('tasks.waiting_approval') }}</option>
          <option value="completed">{{ t('common.completed') }}</option>
          <option value="failed">{{ t('common.failed') }}</option>
          <option value="cancelled">{{ t('common.cancelled') }}</option>
        </select>
      </div>
    </div>

    <div class="card">
      <table v-if="taskStore.tasks.length">
        <thead>
          <tr>
            <th>{{ t('tasks.id') }}</th>
            <th>{{ t('tasks.flow') }}</th>
            <th>{{ t('tasks.state') }}</th>
            <th>{{ t('tasks.status') }}</th>
            <th>{{ t('tasks.created') }}</th>
            <th>{{ t('tasks.actions') }}</th>
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
            <td>
              <button class="btn btn-primary" @click="router.push(`/tasks/${task.id}`)">
                {{ t('common.view') }}
              </button>
            </td>
          </tr>
        </tbody>
      </table>
      <p v-else class="empty">{{ t('tasks.no_tasks') }}</p>
      <Pagination
        v-if="taskStore.tasks.length"
        :total="taskStore.tasks.length"
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
.header-actions {
  display: flex;
  gap: 0.5rem;
  align-items: center;
}
.filter-select {
  padding: 0.375rem 0.5rem;
  border: 1px solid var(--border-color);
  border-radius: 6px;
  font-size: 0.875rem;
  background: white;
}
.empty {
  color: var(--text-secondary);
  padding: 2rem 0;
  text-align: center;
}
</style>
