<script setup lang="ts">
import { onMounted, ref, computed } from 'vue'
import { useApprovalStore } from '../stores/approval'
import { useI18n } from 'vue-i18n'
import Pagination from '../components/common/Pagination.vue'
import type { Approval } from '../types/api'

const { t } = useI18n()
const store = useApprovalStore()

const statusFilter = ref('')
const selectedApproval = ref<Approval | null>(null)
const comment = ref('')
const currentPage = ref(1)
const pageSize = ref(20)

const paginatedApprovals = computed(() => {
  const start = (currentPage.value - 1) * pageSize.value
  return store.approvals.slice(start, start + pageSize.value)
})

function applyFilter() {
  currentPage.value = 1
  store.fetchApprovals(statusFilter.value || undefined)
}

function selectApproval(approval: Approval) {
  if (selectedApproval.value?.id === approval.id) {
    selectedApproval.value = null
    return
  }
  selectedApproval.value = approval
  comment.value = ''
}

function riskClass(level: string): string {
  if (level === 'high') return 'status-failed'
  if (level === 'medium') return 'status-running'
  return 'status-completed'
}

function statusClass(status: string): string {
  if (status === 'approved') return 'status-completed'
  if (status === 'denied') return 'status-failed'
  if (status === 'changes_requested') return 'status-running'
  return 'status-cancelled'
}

async function handleApprove(id: string) {
  await store.approve(id, comment.value || undefined)
  selectedApproval.value = null
  comment.value = ''
}

async function handleDeny(id: string) {
  await store.deny(id, comment.value || undefined)
  selectedApproval.value = null
  comment.value = ''
}

async function handleRequestChanges(id: string) {
  await store.requestChanges(id, comment.value || undefined)
  selectedApproval.value = null
  comment.value = ''
}

onMounted(() => store.fetchApprovals())
</script>

<template>
  <div class="approvals-view">
    <div class="page-header">
      <h2>{{ t('approvals.title') }}</h2>
      <div class="header-actions">
        <select v-model="statusFilter" class="filter-select" @change="applyFilter">
          <option value="">{{ t('common.all') }}</option>
          <option value="pending">{{ t('approvals.pending') }}</option>
          <option value="approved">{{ t('approvals.approved') }}</option>
          <option value="denied">{{ t('approvals.denied') }}</option>
        </select>
      </div>
    </div>

    <div class="card">
      <table v-if="store.approvals.length">
        <thead>
          <tr>
            <th>{{ t('tasks.id') }}</th>
            <th>{{ t('approvals.tool_name') }}</th>
            <th>{{ t('approvals.risk_level') }}</th>
            <th>{{ t('common.status') }}</th>
            <th>{{ t('approvals.requested_at') }}</th>
          </tr>
        </thead>
        <tbody>
          <tr
            v-for="approval in paginatedApprovals"
            :key="approval.id"
            :class="{ 'row-selected': selectedApproval?.id === approval.id }"
            @click="selectApproval(approval)"
            style="cursor: pointer"
          >
            <td class="mono">{{ approval.task_id.slice(0, 8) }}</td>
            <td>{{ approval.tool_name }}</td>
            <td>
              <span :class="['status-badge', riskClass(approval.risk_level)]">
                {{ approval.risk_level }}
              </span>
            </td>
            <td>
              <span :class="['status-badge', statusClass(approval.status)]">
                {{ approval.status }}
              </span>
            </td>
            <td>{{ new Date(approval.requested_at).toLocaleString() }}</td>
          </tr>
        </tbody>
      </table>
      <p v-else class="empty">{{ t('approvals.no_approvals') }}</p>
      <Pagination
        v-if="store.approvals.length"
        :total="store.approvals.length"
        :page-size="pageSize"
        v-model:current-page="currentPage"
      />
    </div>

    <!-- Detail panel -->
    <div v-if="selectedApproval" class="card" style="margin-top: 1rem">
      <div style="display: flex; justify-content: space-between; align-items: center; margin-bottom: 0.75rem">
        <h3 style="font-size: 0.9375rem">{{ selectedApproval.tool_name }}</h3>
        <span class="mono" style="font-size: 0.75rem; color: var(--text-secondary)">{{ selectedApproval.id }}</span>
      </div>
      <div class="detail-grid">
        <div><span class="detail-label">{{ t('tasks.id') }}</span> <span class="mono">{{ selectedApproval.task_id }}</span></div>
        <div><span class="detail-label">{{ t('approvals.risk_level') }}</span> <span :class="['status-badge', riskClass(selectedApproval.risk_level)]">{{ selectedApproval.risk_level }}</span></div>
        <div><span class="detail-label">{{ t('common.status') }}</span> <span :class="['status-badge', statusClass(selectedApproval.status)]">{{ selectedApproval.status }}</span></div>
        <div><span class="detail-label">{{ t('approvals.requested_at') }}</span> {{ new Date(selectedApproval.requested_at).toLocaleString() }}</div>
        <div v-if="selectedApproval.decided_at"><span class="detail-label">{{ t('approvals.decided_at') }}</span> {{ new Date(selectedApproval.decided_at).toLocaleString() }}</div>
        <div v-if="selectedApproval.decided_by"><span class="detail-label">{{ t('approvals.decided_by') }}</span> {{ selectedApproval.decided_by }}</div>
      </div>

      <!-- Actions for pending approvals -->
      <div v-if="selectedApproval.status === 'pending'" class="approval-actions">
        <input
          v-model="comment"
          class="comment-input"
          :placeholder="t('approval.comment_placeholder')"
        />
        <div class="action-buttons">
          <button class="btn btn-primary btn-sm" @click="handleApprove(selectedApproval!.id)">
            {{ t('approval.approve') }}
          </button>
          <button class="btn btn-secondary btn-sm" @click="handleRequestChanges(selectedApproval!.id)">
            {{ t('approval.request_changes') }}
          </button>
          <button class="btn btn-danger btn-sm" @click="handleDeny(selectedApproval!.id)">
            {{ t('approval.deny') }}
          </button>
        </div>
      </div>
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
.header-actions {
  display: flex;
  gap: 0.5rem;
  align-items: center;
}
.filter-select {
  padding: 0.375rem 0.75rem;
  border: 1px solid var(--border-color);
  border-radius: 6px;
  background: var(--bg-surface);
  color: var(--text-primary);
  font-size: 0.8125rem;
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
.empty {
  color: var(--text-secondary);
  padding: 2rem 0;
  text-align: center;
}
.approval-actions {
  margin-top: 1rem;
  padding-top: 0.75rem;
  border-top: 1px solid var(--border-color);
}
.comment-input {
  width: 100%;
  padding: 0.5rem;
  border: 1px solid var(--border-color);
  border-radius: 6px;
  font-size: 0.875rem;
  background: var(--bg-surface);
  color: var(--text-primary);
  margin-bottom: 0.75rem;
}
.action-buttons {
  display: flex;
  gap: 0.5rem;
}
</style>
