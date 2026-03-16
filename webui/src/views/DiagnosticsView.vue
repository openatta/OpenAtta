<script setup lang="ts">
import { ref, computed } from 'vue'
import { useApi } from '../composables/useApi'
import { useI18n } from 'vue-i18n'
import type { DiagResult } from '../types/api'

const { t } = useI18n()
const api = useApi()
const results = ref<DiagResult[]>([])
const loading = ref(false)
const hasRun = ref(false)

async function runDiag() {
  loading.value = true
  hasRun.value = true
  try {
    const resp = await api.post<{ data: DiagResult[] }>('/api/v1/diagnostics/run', {})
    results.value = resp.data
  } catch (e: any) {
    results.value = [{ severity: 'error', category: 'system', message: e.message || t('error.diagnostics_failed') }]
  }
  loading.value = false
}

const counts = computed(() => {
  const ok = results.value.filter(r => r.severity === 'ok').length
  const warn = results.value.filter(r => r.severity === 'warn').length
  const error = results.value.filter(r => r.severity === 'error').length
  return { ok, warn, error }
})

const grouped = computed(() => {
  const map: Record<string, DiagResult[]> = {}
  for (const r of results.value) {
    if (!map[r.category]) map[r.category] = []
    map[r.category].push(r)
  }
  return map
})

const severityIcon: Record<string, string> = {
  ok: '✓',
  warn: '⚠',
  error: '✗',
}

const severityColor: Record<string, string> = {
  ok: 'var(--color-success)',
  warn: 'var(--color-warning)',
  error: 'var(--color-error)',
}
</script>

<template>
  <div class="diag-view">
    <div class="header-row">
      <h2>{{ t('diagnostics.title') }}</h2>
      <button class="btn btn-primary" @click="runDiag" :disabled="loading">
        {{ loading ? t('diagnostics.running') : t('diagnostics.run') }}
      </button>
    </div>

    <!-- Summary -->
    <div v-if="hasRun && !loading" class="summary-bar">
      <span class="summary-item" style="color: var(--color-success)">{{ counts.ok }} {{ t('common.ok') }}</span>
      <span class="summary-item" style="color: var(--color-warning)">{{ counts.warn }} {{ t('common.warn') }}</span>
      <span class="summary-item" style="color: var(--color-error)">{{ counts.error }} {{ t('common.error') }}</span>
    </div>

    <!-- Results grouped by category -->
    <div v-if="hasRun && !loading">
      <div v-for="(items, category) in grouped" :key="category" class="card diag-group">
        <h3 class="group-title">{{ category }}</h3>
        <div v-for="(item, i) in items" :key="i" class="diag-item">
          <span class="diag-icon" :style="{ color: severityColor[item.severity] }">
            {{ severityIcon[item.severity] }}
          </span>
          <span class="diag-msg">{{ item.message }}</span>
          <span :class="['status-badge', item.severity === 'ok' ? 'status-completed' : item.severity === 'warn' ? 'status-waiting_approval' : 'status-failed']">
            {{ item.severity }}
          </span>
        </div>
      </div>
    </div>

    <div v-else-if="!hasRun" class="card">
      <p class="empty">{{ t('diagnostics.empty_hint') }}</p>
    </div>

    <div v-else-if="loading" class="card">
      <p class="empty">{{ t('diagnostics.running_hint') }}</p>
    </div>
  </div>
</template>

<style scoped>
.diag-view h2 { margin-bottom: 0; }
.summary-bar {
  display: flex;
  gap: 1.5rem;
  margin-bottom: 1rem;
  font-size: 1rem;
  font-weight: 600;
}
.summary-item {
  display: flex;
  align-items: center;
  gap: 0.25rem;
}
.diag-group {
  margin-bottom: 0.75rem;
}
.group-title {
  font-size: 0.875rem;
  margin-bottom: 0.5rem;
  text-transform: capitalize;
}
.diag-item {
  display: flex;
  align-items: center;
  gap: 0.75rem;
  padding: 0.5rem 0;
  border-bottom: 1px solid var(--border-color);
  font-size: 0.875rem;
}
.diag-item:last-child { border-bottom: none; }
.diag-icon {
  font-size: 1rem;
  font-weight: 700;
  min-width: 1.25rem;
  text-align: center;
}
.diag-msg {
  flex: 1;
}
</style>
