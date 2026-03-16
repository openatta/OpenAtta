<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { useFlowStore } from '../stores/flow'
import { useI18n } from 'vue-i18n'
import FlowGraph from '../components/flow/FlowGraph.vue'
import Pagination from '../components/common/Pagination.vue'
import type { FlowDef } from '../types/api'

const { t } = useI18n()
const flowStore = useFlowStore()
const selectedFlow = ref<FlowDef | null>(null)
const viewMode = ref<'graph' | 'yaml'>('graph')
const showImport = ref(false)
const importText = ref('')
const importError = ref('')
const currentPage = ref(1)
const pageSize = ref(20)

const paginatedFlows = computed(() => {
  const start = (currentPage.value - 1) * pageSize.value
  return flowStore.flows.slice(start, start + pageSize.value)
})

function selectFlow(flow: FlowDef, mode: 'graph' | 'yaml') {
  if (selectedFlow.value?.id === flow.id && viewMode.value === mode) {
    selectedFlow.value = null
  } else {
    selectedFlow.value = flow
    viewMode.value = mode
  }
}

const yamlText = computed(() => {
  if (!selectedFlow.value) return ''
  return flowDefToYaml(selectedFlow.value)
})

function flowDefToYaml(flow: FlowDef): string {
  const lines: string[] = []
  lines.push(`id: ${flow.id}`)
  lines.push(`version: "${flow.version}"`)
  if (flow.name) lines.push(`name: "${flow.name}"`)
  if (flow.description) lines.push(`description: "${flow.description}"`)
  if (flow.skills?.length) {
    lines.push('skills:')
    for (const s of flow.skills) lines.push(`  - ${s}`)
  }
  lines.push(`initial_state: ${flow.initial_state}`)
  lines.push('states:')
  for (const [id, state] of Object.entries(flow.states)) {
    lines.push(`  ${id}:`)
    lines.push(`    type: ${state.type}`)
    if (state.skill) lines.push(`    skill: ${state.skill}`)
    if (state.agent) lines.push(`    agent: ${state.agent}`)
    if ((state as any).gate) {
      const gate = (state as any).gate
      lines.push('    gate:')
      if (gate.approver_role) lines.push(`      approver_role: ${gate.approver_role}`)
      if (gate.timeout) lines.push(`      timeout: "${gate.timeout}"`)
      if (gate.on_timeout) lines.push(`      on_timeout: ${gate.on_timeout}`)
    }
    if ((state as any).on_enter?.length) {
      lines.push('    on_enter:')
      for (const action of (state as any).on_enter) {
        lines.push(`      - type: ${action.type}`)
        for (const [k, v] of Object.entries(action)) {
          if (k !== 'type') lines.push(`        ${k}: ${JSON.stringify(v)}`)
        }
      }
    }
    if (state.transitions?.length) {
      lines.push('    transitions:')
      for (const tr of state.transitions) {
        lines.push(`      - to: ${tr.to}`)
        if (tr.when) lines.push(`        when: "${tr.when}"`)
        if (tr.auto) lines.push(`        auto: true`)
      }
    }
  }
  return lines.join('\n')
}

async function importFlow() {
  importError.value = ''
  try {
    const parsed = JSON.parse(importText.value)
    await flowStore.createFlow(parsed)
    importText.value = ''
    showImport.value = false
    currentPage.value = 1
    await flowStore.fetchFlows()
  } catch (e: any) {
    importError.value = e.message || t('error.failed_import_flow')
  }
}

async function deleteFlow(id: string) {
  if (!confirm(t('confirm.delete_flow', { id }))) return
  try {
    await flowStore.deleteFlow(id)
    if (selectedFlow.value?.id === id) selectedFlow.value = null
    currentPage.value = 1
    await flowStore.fetchFlows()
  } catch {
    // Error toast shown by global onResponseError handler
  }
}

onMounted(() => flowStore.fetchFlows())
</script>

<template>
  <div class="flows-view">
    <div class="header-row">
      <h2>{{ t('flows.title') }}</h2>
      <button class="btn btn-primary" @click="showImport = !showImport">
        {{ showImport ? t('common.cancel') : t('flows.import_flow') }}
      </button>
    </div>

    <!-- Import panel -->
    <div v-if="showImport" class="card import-panel">
      <h3>{{ t('flows.import_title') }}</h3>
      <p class="hint">{{ t('flows.import_hint') }}</p>
      <textarea v-model="importText" rows="8" placeholder='{"id": "my-flow", "version": "1.0", ...}'></textarea>
      <p v-if="importError" class="error-text">{{ importError }}</p>
      <button class="btn btn-primary" @click="importFlow" :disabled="!importText.trim()">{{ t('flows.import') }}</button>
    </div>

    <div class="card">
      <table v-if="flowStore.flows.length">
        <thead>
          <tr>
            <th>{{ t('flows.id') }}</th>
            <th>{{ t('flows.name') }}</th>
            <th>{{ t('flows.version') }}</th>
            <th>{{ t('flows.source') }}</th>
            <th>{{ t('flows.states') }}</th>
            <th>{{ t('flows.initial') }}</th>
            <th>{{ t('flows.actions') }}</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="flow in paginatedFlows" :key="flow.id">
            <td class="mono">{{ flow.id }}</td>
            <td>{{ flow.name || '-' }}</td>
            <td>{{ flow.version }}</td>
            <td>
              <span :class="['source-badge', flow.source === 'imported' ? 'source-imported' : 'source-builtin']">
                {{ flow.source ? flow.source : t('common.source_builtin') }}
              </span>
            </td>
            <td>{{ Object.keys(flow.states).length }}</td>
            <td>{{ flow.initial_state }}</td>
            <td>
              <div class="action-buttons">
                <button
                  class="btn btn-sm"
                  :class="selectedFlow?.id === flow.id && viewMode === 'graph' ? 'btn-active' : 'btn-primary'"
                  @click="selectFlow(flow, 'graph')"
                >{{ t('flows.graph') }}</button>
                <button
                  class="btn btn-sm"
                  :class="selectedFlow?.id === flow.id && viewMode === 'yaml' ? 'btn-active' : 'btn-outline'"
                  @click="selectFlow(flow, 'yaml')"
                >{{ t('flows.yaml') }}</button>
                <button v-if="flow.source === 'imported'" class="btn btn-sm btn-danger" @click="deleteFlow(flow.id)">
                  {{ t('common.delete') }}
                </button>
              </div>
            </td>
          </tr>
        </tbody>
      </table>
      <p v-else class="empty">{{ t('flows.no_flows') }}</p>
      <Pagination
        v-if="flowStore.flows.length"
        :total="flowStore.flows.length"
        :page-size="pageSize"
        v-model:current-page="currentPage"
      />
    </div>

    <!-- Flow detail panel -->
    <div v-if="selectedFlow" class="card detail-card">
      <div class="detail-header">
        <h3>{{ selectedFlow.name || selectedFlow.id }}</h3>
        <p v-if="selectedFlow.description" class="description">{{ selectedFlow.description }}</p>
        <div class="view-tabs">
          <button
            :class="['tab', viewMode === 'graph' ? 'tab-active' : '']"
            @click="viewMode = 'graph'"
          >{{ t('flows.graph') }}</button>
          <button
            :class="['tab', viewMode === 'yaml' ? 'tab-active' : '']"
            @click="viewMode = 'yaml'"
          >{{ t('flows.yaml') }}</button>
        </div>
      </div>

      <FlowGraph v-if="viewMode === 'graph'" :flow="selectedFlow" />

      <div v-else class="yaml-panel">
        <pre><code>{{ yamlText }}</code></pre>
      </div>
    </div>
  </div>
</template>

<style scoped>
.flows-view h2 { margin-bottom: 0; }
.header-row { display: flex; justify-content: space-between; align-items: center; margin-bottom: 1.25rem; }
.import-panel { margin-bottom: 1rem; }
.import-panel h3 { font-size: 0.875rem; margin-bottom: 0.5rem; }
.hint { color: var(--text-secondary); font-size: 0.75rem; margin-bottom: 0.5rem; }
.import-panel textarea {
  width: 100%;
  font-family: monospace;
  font-size: 0.8125rem;
  padding: 0.5rem;
  border: 1px solid var(--border-color);
  border-radius: 6px;
  background: var(--bg-page);
  color: var(--text-primary);
  resize: vertical;
  margin-bottom: 0.5rem;
}
.error-text { color: #ef4444; font-size: 0.75rem; margin-bottom: 0.5rem; }
table { width: 100%; border-collapse: collapse; }
th, td { text-align: left; padding: 0.5rem 0.75rem; border-bottom: 1px solid var(--border-color); }
th { font-weight: 600; font-size: 0.8125rem; color: var(--text-secondary); }
.mono { font-family: monospace; }
.action-buttons { display: flex; gap: 0.375rem; }
.btn-sm { padding: 0.25rem 0.5rem; font-size: 0.75rem; }
.btn-danger { background: #ef4444; color: #fff; border: none; border-radius: 4px; cursor: pointer; }
.btn-danger:hover { background: #dc2626; }
.btn-outline { background: transparent; border: 1px solid var(--border-color); color: var(--text-primary); border-radius: 4px; cursor: pointer; }
.btn-outline:hover { background: var(--bg-page); }
.btn-active { background: #2563eb; color: #fff; border: 1px solid #2563eb; border-radius: 4px; cursor: pointer; }
.source-badge { font-size: 0.6875rem; padding: 0.0625rem 0.375rem; border-radius: 4px; font-weight: 500; }
.source-builtin { background: #dbeafe; color: #1d4ed8; }
.source-imported { background: #fef3c7; color: #92400e; }
.detail-card { margin-top: 1.25rem; }
.detail-header { margin-bottom: 0.75rem; }
.detail-header h3 { margin-bottom: 0.25rem; font-size: 0.9375rem; }
.description { color: var(--text-secondary); font-size: 0.8125rem; margin-bottom: 0.75rem; }
.view-tabs { display: flex; gap: 0; border-bottom: 1px solid var(--border-color); margin-bottom: 0.75rem; }
.tab {
  padding: 0.375rem 0.75rem;
  font-size: 0.8125rem;
  background: none;
  border: none;
  border-bottom: 2px solid transparent;
  color: var(--text-secondary);
  cursor: pointer;
}
.tab:hover { color: var(--text-primary); }
.tab-active { color: #2563eb; border-bottom-color: #2563eb; font-weight: 500; }
.yaml-panel {
  background: var(--bg-code, #1e1e2e);
  border-radius: 8px;
  overflow: auto;
  max-height: 500px;
}
.yaml-panel pre {
  margin: 0;
  padding: 1rem;
  font-size: 0.8125rem;
  line-height: 1.6;
  color: var(--text-code, #cdd6f4);
  white-space: pre;
}
.empty { color: var(--text-secondary); padding: 2rem 0; text-align: center; }
</style>
