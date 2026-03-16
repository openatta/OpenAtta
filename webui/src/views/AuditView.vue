<script setup lang="ts">
import { onMounted, ref, computed } from 'vue'
import { useAuditStore } from '../stores/audit'
import { useI18n } from 'vue-i18n'

const { t } = useI18n()
const store = useAuditStore()

const actorTypeFilter = ref('')
const actionFilter = ref('')
const currentPage = ref(0)
const pageSize = 50

const totalPages = computed(() => Math.max(1, Math.ceil(store.total / pageSize)))

function applyFilter() {
  currentPage.value = 0
  fetchPage()
}

function fetchPage() {
  store.fetchEntries({
    actor_type: actorTypeFilter.value || undefined,
    action: actionFilter.value || undefined,
    limit: pageSize,
    offset: currentPage.value * pageSize,
  })
}

function prevPage() {
  if (currentPage.value > 0) {
    currentPage.value--
    fetchPage()
  }
}

function nextPage() {
  if (currentPage.value < totalPages.value - 1) {
    currentPage.value++
    fetchPage()
  }
}

function exportCsv() {
  store.exportCsv()
}

onMounted(() => fetchPage())
</script>

<template>
  <div class="audit-view">
    <div class="page-header">
      <h2>{{ t('audit.title') }}</h2>
      <div class="header-actions">
        <input
          v-model="actorTypeFilter"
          class="filter-input"
          :placeholder="t('audit.filter_by_actor')"
          @keyup.enter="applyFilter"
        />
        <input
          v-model="actionFilter"
          class="filter-input"
          :placeholder="t('audit.filter_by_action')"
          @keyup.enter="applyFilter"
        />
        <button class="btn btn-secondary" @click="applyFilter">{{ t('common.search') }}</button>
        <button class="btn btn-primary" @click="exportCsv">{{ t('audit.export') }}</button>
      </div>
    </div>

    <div class="card">
      <table v-if="store.entries.length">
        <thead>
          <tr>
            <th>{{ t('audit.timestamp') }}</th>
            <th>{{ t('audit.actor') }}</th>
            <th>{{ t('audit.action') }}</th>
            <th>{{ t('audit.resource') }}</th>
            <th>{{ t('audit.result') }}</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="entry in store.entries" :key="entry.id">
            <td>{{ new Date(entry.timestamp).toLocaleString() }}</td>
            <td>
              <span class="mono">{{ entry.actor.actor_type }}</span>
              <span class="text-secondary"> / {{ entry.actor.id.slice(0, 8) }}</span>
            </td>
            <td>{{ entry.action }}</td>
            <td>
              <span class="mono">{{ entry.resource_type }}</span>
              <span class="text-secondary"> / {{ entry.resource_id.slice(0, 8) }}</span>
            </td>
            <td>
              <span :class="['status-badge', entry.result === 'success' ? 'status-completed' : 'status-failed']">
                {{ entry.result }}
              </span>
            </td>
          </tr>
        </tbody>
      </table>
      <p v-else class="empty">{{ t('audit.no_entries') }}</p>
    </div>

    <div v-if="store.entries.length" class="pagination">
      <button class="btn btn-secondary btn-sm" :disabled="currentPage === 0" @click="prevPage">&laquo;</button>
      <span class="page-info">{{ currentPage + 1 }} / {{ totalPages }}</span>
      <button class="btn btn-secondary btn-sm" :disabled="currentPage >= totalPages - 1" @click="nextPage">&raquo;</button>
    </div>
  </div>
</template>

<style scoped>
.page-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 1.25rem;
  flex-wrap: wrap;
  gap: 0.5rem;
}
.header-actions {
  display: flex;
  gap: 0.5rem;
  align-items: center;
}
.filter-input {
  padding: 0.375rem 0.5rem;
  border: 1px solid var(--border-color);
  border-radius: 6px;
  font-size: 0.875rem;
  background: var(--bg-surface);
  color: var(--text-primary);
  width: 160px;
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
.text-secondary { color: var(--text-secondary); }
.empty {
  color: var(--text-secondary);
  padding: 2rem 0;
  text-align: center;
}
.pagination {
  display: flex;
  justify-content: center;
  align-items: center;
  gap: 0.75rem;
  margin-top: 1rem;
}
.page-info {
  font-size: 0.875rem;
  color: var(--text-secondary);
}
</style>
