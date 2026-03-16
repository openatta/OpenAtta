<script setup lang="ts">
import { onMounted, computed } from 'vue'
import { useRoute } from 'vue-router'
import { useTaskStore } from '../stores/task'
import { useI18n } from 'vue-i18n'
import ApprovalActions from '../components/task/ApprovalActions.vue'

const { t } = useI18n()
const route = useRoute()
const taskStore = useTaskStore()

function translateStatus(status: string): string {
  const key = `common.status_${status}`
  const translated = t(key)
  return translated !== key ? translated : status
}

const isWaitingApproval = computed(() => {
  const status = taskStore.currentTask?.status
  return status === 'waiting_approval'
})

function onApprovalDecided() {
  const id = route.params.id as string
  taskStore.fetchTask(id)
}

onMounted(() => {
  const id = route.params.id as string
  taskStore.fetchTask(id)
})
</script>

<template>
  <div class="task-detail">
    <h2>{{ t('task_detail.title') }}</h2>

    <div v-if="taskStore.currentTask" class="card">
      <div class="detail-grid">
        <div class="field">
          <label>{{ t('task_detail.id') }}</label>
          <span class="mono">{{ taskStore.currentTask.id }}</span>
        </div>
        <div class="field">
          <label>{{ t('task_detail.flow') }}</label>
          <span>{{ taskStore.currentTask.flow_id }}</span>
        </div>
        <div class="field">
          <label>{{ t('task_detail.current_state') }}</label>
          <span>{{ taskStore.currentTask.current_state }}</span>
        </div>
        <div class="field">
          <label>{{ t('task_detail.status') }}</label>
          <span :class="['status-badge', `status-${typeof taskStore.currentTask.status === 'string' ? taskStore.currentTask.status : 'failed'}`]">
            {{ translateStatus(typeof taskStore.currentTask.status === 'string' ? taskStore.currentTask.status : 'failed') }}
          </span>
        </div>
        <div class="field">
          <label>{{ t('task_detail.created') }}</label>
          <span>{{ new Date(taskStore.currentTask.created_at).toLocaleString() }}</span>
        </div>
      </div>

      <div class="section">
        <h3>{{ t('task_detail.input') }}</h3>
        <pre class="code-block">{{ JSON.stringify(taskStore.currentTask.input, null, 2) }}</pre>
      </div>

      <div v-if="taskStore.currentTask.output" class="section">
        <h3>{{ t('task_detail.output') }}</h3>
        <pre class="code-block">{{ JSON.stringify(taskStore.currentTask.output, null, 2) }}</pre>
      </div>

      <div class="section">
        <h3>{{ t('task_detail.state_data') }}</h3>
        <pre class="code-block">{{ JSON.stringify(taskStore.currentTask.state_data, null, 2) }}</pre>
      </div>

      <ApprovalActions
        v-if="isWaitingApproval"
        :approval-id="taskStore.currentTask.id"
        @decided="onApprovalDecided"
      />
    </div>

    <div v-else class="card">
      <p class="empty">{{ t('task_detail.loading') }}</p>
    </div>
  </div>
</template>

<style scoped>
.task-detail h2 {
  margin-bottom: 1.25rem;
}
.detail-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(250px, 1fr));
  gap: 1rem;
  margin-bottom: 1.5rem;
}
.field label {
  display: block;
  font-size: 0.75rem;
  font-weight: 600;
  color: var(--text-secondary);
  margin-bottom: 0.25rem;
  text-transform: uppercase;
}
.mono { font-family: monospace; }
.section {
  margin-top: 1.5rem;
}
.section h3 {
  font-size: 0.875rem;
  margin-bottom: 0.5rem;
}
.code-block {
  background: #f8f9fa;
  border: 1px solid var(--border-color);
  border-radius: 6px;
  padding: 0.75rem;
  font-family: monospace;
  font-size: 0.8125rem;
  overflow-x: auto;
  white-space: pre-wrap;
}
.empty {
  color: var(--text-secondary);
  text-align: center;
  padding: 2rem;
}
</style>
