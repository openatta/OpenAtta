<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { useCronStore } from '../stores/cron'
import { useI18n } from 'vue-i18n'
import type { CronJob, CronRun } from '../types/api'
import Pagination from '../components/common/Pagination.vue'

const { t } = useI18n()
const store = useCronStore()

function translateStatus(status: string): string {
  const key = `common.status_${status}`
  const translated = t(key)
  return translated !== key ? translated : status
}
const selectedJob = ref<CronJob | null>(null)
const runs = ref<CronRun[]>([])
const showCreate = ref(false)
const form = ref({ name: '', schedule: '', command: '', config: '{}' })
const formError = ref('')
const filter = ref<'all' | 'enabled' | 'disabled'>('all')
const currentPage = ref(1)
const pageSize = ref(20)

onMounted(() => store.fetchJobs())

async function selectJob(job: CronJob) {
  if (selectedJob.value?.id === job.id) {
    selectedJob.value = null
    return
  }
  selectedJob.value = job
  try {
    const resp = await store.fetchRuns(job.id)
    runs.value = resp.data
  } catch {
    runs.value = []
  }
}

async function createJob() {
  formError.value = ''
  try {
    let config = {}
    if (form.value.config.trim()) {
      config = JSON.parse(form.value.config)
    }
    await store.createJob({
      name: form.value.name,
      schedule: form.value.schedule,
      command: form.value.command,
      config,
    })
    showCreate.value = false
    form.value = { name: '', schedule: '', command: '', config: '{}' }
    await store.fetchJobs()
  } catch (e: any) {
    formError.value = e.message || t('error.failed_create_job')
  }
}

async function toggleEnabled(job: CronJob) {
  await store.updateJob(job.id, { enabled: !job.enabled })
  await store.fetchJobs()
  if (selectedJob.value?.id === job.id) {
    selectedJob.value = store.jobs.find(j => j.id === job.id) || null
  }
}

async function triggerNow(job: CronJob) {
  await store.triggerJob(job.id)
  if (selectedJob.value?.id === job.id) {
    const resp = await store.fetchRuns(job.id)
    runs.value = resp.data
  }
}

async function deleteJob(job: CronJob) {
  if (!confirm(t('confirm.delete_cron_job', { name: job.name }))) return
  await store.deleteJob(job.id)
  if (selectedJob.value?.id === job.id) selectedJob.value = null
  await store.fetchJobs()
}

function relativeTime(dateStr?: string): string {
  if (!dateStr) return '-'
  const diff = new Date(dateStr).getTime() - Date.now()
  const abs = Math.abs(diff)
  if (abs < 60000) return diff > 0 ? t('time.in_seconds') : t('time.seconds_ago')
  if (abs < 3600000) {
    const m = Math.round(abs / 60000)
    return diff > 0 ? t('time.in_minutes', { n: m }) : t('time.minutes_ago', { n: m })
  }
  if (abs < 86400000) {
    const h = Math.round(abs / 3600000)
    return diff > 0 ? t('time.in_hours', { n: h }) : t('time.hours_ago', { n: h })
  }
  const d = Math.round(abs / 86400000)
  return diff > 0 ? t('time.in_days', { n: d }) : t('time.days_ago', { n: d })
}

function formatDuration(start: string, end?: string): string {
  if (!end) return t('time.running')
  const ms = new Date(end).getTime() - new Date(start).getTime()
  if (ms < 1000) return `${ms}ms`
  return `${(ms / 1000).toFixed(1)}s`
}

function formatTime(ts: string): string {
  try {
    return new Date(ts).toLocaleTimeString()
  } catch {
    return ts
  }
}

const filteredJobs = computed(() => {
  if (filter.value === 'enabled') return store.jobs.filter(j => j.enabled)
  if (filter.value === 'disabled') return store.jobs.filter(j => !j.enabled)
  return store.jobs
})

const paginatedJobs = computed(() => {
  const start = (currentPage.value - 1) * pageSize.value
  return filteredJobs.value.slice(start, start + pageSize.value)
})
</script>

<template>
  <div class="cron-view">
    <div class="header-row">
      <h2>{{ t('cron.title') }}</h2>
      <div class="action-buttons">
        <select v-model="filter" class="filter-select" @change="currentPage = 1; store.fetchJobs()">
          <option value="all">{{ t('common.all') }}</option>
          <option value="enabled">{{ t('common.enabled') }}</option>
          <option value="disabled">{{ t('common.disabled') }}</option>
        </select>
        <button class="btn btn-primary" @click="showCreate = !showCreate">
          {{ showCreate ? t('common.cancel') : t('cron.new_job') }}
        </button>
      </div>
    </div>

    <!-- Create form -->
    <div v-if="showCreate" class="card" style="margin-bottom: 1rem">
      <h3 style="font-size: 0.875rem; margin-bottom: 0.75rem">{{ t('cron.new_title') }}</h3>
      <div class="form-grid">
        <div class="form-field">
          <label>{{ t('common.name') }}</label>
          <input v-model="form.name" placeholder="cleanup-old-tasks" />
        </div>
        <div class="form-field">
          <label>{{ t('cron.schedule_label') }}</label>
          <input v-model="form.schedule" placeholder="0 0 * * *" class="mono" />
        </div>
        <div class="form-field">
          <label>{{ t('cron.command') }}</label>
          <input v-model="form.command" placeholder="delete-old-tasks" />
        </div>
        <div class="form-field">
          <label>{{ t('cron.config_label') }}</label>
          <input v-model="form.config" placeholder="{}" class="mono" />
        </div>
      </div>
      <p v-if="formError" style="color: var(--color-error); font-size: 0.75rem; margin: 0.5rem 0">{{ formError }}</p>
      <button class="btn btn-primary btn-sm" @click="createJob" :disabled="!form.name || !form.schedule || !form.command">
        {{ t('common.create') }}
      </button>
    </div>

    <!-- Jobs table -->
    <div class="card">
      <table v-if="filteredJobs.length">
        <thead>
          <tr>
            <th>{{ t('common.name') }}</th>
            <th>{{ t('cron.schedule') }}</th>
            <th>{{ t('cron.next_run') }}</th>
            <th>{{ t('cron.last_run') }}</th>
            <th>{{ t('common.status') }}</th>
            <th>{{ t('common.actions') }}</th>
          </tr>
        </thead>
        <tbody>
          <tr
            v-for="job in paginatedJobs"
            :key="job.id"
            :class="{ 'row-selected': selectedJob?.id === job.id }"
            @click="selectJob(job)"
            style="cursor: pointer"
          >
            <td class="mono">{{ job.name }}</td>
            <td class="mono" style="font-size: 0.8125rem">{{ job.schedule }}</td>
            <td>{{ relativeTime(job.next_run_at) }}</td>
            <td>{{ relativeTime(job.last_run_at) }}</td>
            <td>
              <span :class="['status-badge', job.enabled ? 'status-completed' : 'status-cancelled']">
                {{ job.enabled ? t('common.enabled') : t('common.disabled') }}
              </span>
            </td>
            <td @click.stop>
              <div class="action-buttons">
                <button class="btn btn-sm btn-primary" @click="triggerNow(job)" :title="t('cron.run_now')">▶</button>
                <button class="btn btn-sm btn-secondary" @click="toggleEnabled(job)">
                  {{ job.enabled ? t('cron.disable') : t('cron.enable') }}
                </button>
                <button class="btn btn-sm btn-danger" @click="deleteJob(job)">{{ t('common.delete') }}</button>
              </div>
            </td>
          </tr>
        </tbody>
      </table>
      <p v-else class="empty">{{ t('cron.no_jobs') }}</p>
      <Pagination
        v-if="filteredJobs.length"
        :total="filteredJobs.length"
        :page-size="pageSize"
        v-model:current-page="currentPage"
      />
    </div>

    <!-- Job detail + run history -->
    <div v-if="selectedJob" class="card" style="margin-top: 1rem">
      <div style="display: flex; justify-content: space-between; align-items: center; margin-bottom: 0.75rem">
        <h3 style="font-size: 0.9375rem">{{ selectedJob.name }}</h3>
        <span class="mono" style="font-size: 0.75rem; color: var(--text-secondary)">{{ selectedJob.id }}</span>
      </div>
      <div class="detail-grid">
        <div><span class="detail-label">{{ t('cron.schedule') }}</span> <span class="mono">{{ selectedJob.schedule }}</span></div>
        <div><span class="detail-label">{{ t('cron.command') }}</span> <span class="mono">{{ selectedJob.command }}</span></div>
        <div><span class="detail-label">{{ t('cron.created_by') }}</span> {{ selectedJob.created_by }}</div>
        <div><span class="detail-label">{{ t('dashboard.created') }}</span> {{ new Date(selectedJob.created_at).toLocaleString() }}</div>
      </div>

      <h4 style="margin-top: 1rem; margin-bottom: 0.5rem; font-size: 0.875rem">{{ t('cron.run_history') }}</h4>
      <table v-if="runs.length">
        <thead>
          <tr>
            <th>{{ t('cron.time') }}</th>
            <th>{{ t('common.status') }}</th>
            <th>{{ t('cron.duration') }}</th>
            <th>{{ t('cron.triggered_by') }}</th>
            <th>{{ t('cron.output_error') }}</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="run in runs" :key="run.id">
            <td>{{ formatTime(run.started_at) }}</td>
            <td>
              <span :class="['status-badge', run.status === 'completed' ? 'status-completed' : run.status === 'failed' ? 'status-failed' : 'status-running']">
                {{ translateStatus(run.status) }}
              </span>
            </td>
            <td>{{ formatDuration(run.started_at, run.completed_at) }}</td>
            <td>{{ run.triggered_by }}</td>
            <td class="mono" style="font-size: 0.75rem; max-width: 300px; overflow: hidden; text-overflow: ellipsis">
              {{ run.error || run.output || '-' }}
            </td>
          </tr>
        </tbody>
      </table>
      <p v-else class="empty" style="padding: 1rem 0">{{ t('cron.no_runs') }}</p>
    </div>
  </div>
</template>

<style scoped>
.cron-view h2 { margin-bottom: 0; }
.filter-select {
  padding: 0.375rem 0.75rem;
  border: 1px solid var(--border-color);
  border-radius: 6px;
  background: var(--bg-surface);
  color: var(--text-primary);
  font-size: 0.8125rem;
}
.form-grid {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 0.75rem;
  margin-bottom: 0.75rem;
}
.form-field label {
  display: block;
  font-size: 0.75rem;
  color: var(--text-secondary);
  margin-bottom: 0.25rem;
}
.form-field input {
  width: 100%;
}
.row-selected {
  background: var(--bg-hover);
}
.detail-grid {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 0.5rem;
  font-size: 0.875rem;
}
.detail-label {
  color: var(--text-secondary);
  margin-right: 0.5rem;
}
</style>
