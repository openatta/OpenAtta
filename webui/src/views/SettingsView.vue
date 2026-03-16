<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useApi } from '../composables/useApi'
import { useI18n } from 'vue-i18n'
import type { SystemConfig, SecurityPolicy, NodeInfo } from '../types/api'

const { t } = useI18n()
const api = useApi()
const loading = ref(false)
const config = ref<SystemConfig | null>(null)
const policy = ref<SecurityPolicy | null>(null)
const channels = ref<string[]>([])
const nodes = ref<NodeInfo[]>([])
const editing = ref(false)
const editPolicy = ref<SecurityPolicy | null>(null)
const saving = ref(false)
const saveError = ref('')

async function fetchConfig() {
  try {
    config.value = await api.get<SystemConfig>('/api/v1/system/config')
  } catch { /* ignore */ }
}

async function fetchPolicy() {
  try {
    const resp = await api.get<{ data: SecurityPolicy }>('/api/v1/security/policy')
    policy.value = resp.data
  } catch { /* ignore */ }
}

async function fetchChannels() {
  try {
    const resp = await api.get<{ data: string[] }>('/api/v1/channels')
    channels.value = resp.data
  } catch { /* ignore */ }
}

async function fetchNodes() {
  try {
    nodes.value = await api.get<NodeInfo[]>('/api/v1/nodes')
  } catch { /* ignore */ }
}

function startEdit() {
  if (!policy.value) return
  editPolicy.value = { ...policy.value }
  editing.value = true
  saveError.value = ''
}

function cancelEdit() {
  editing.value = false
  editPolicy.value = null
  saveError.value = ''
}

async function savePolicy() {
  if (!editPolicy.value) return
  saving.value = true
  saveError.value = ''
  try {
    await api.put('/api/v1/security/policy', editPolicy.value as any)
    policy.value = { ...editPolicy.value }
    editing.value = false
    editPolicy.value = null
  } catch (e: any) {
    saveError.value = e.message || t('error.failed_save_policy')
  }
  saving.value = false
}

onMounted(async () => {
  loading.value = true
  await Promise.all([fetchConfig(), fetchPolicy(), fetchChannels(), fetchNodes()])
  loading.value = false
})

function translateStatus(status: string): string {
  const key = `common.status_${status}`
  const translated = t(key)
  return translated !== key ? translated : status
}

function translateAutonomy(level: string): string {
  const key = `settings.autonomy_${level}`
  const translated = t(key)
  return translated !== key ? translated : level
}

function formatBytes(bytes: number): string {
  if (bytes === 0) return '0 B'
  const k = 1024
  const sizes = ['B', 'KB', 'MB', 'GB']
  const i = Math.floor(Math.log(bytes) / Math.log(k))
  return parseFloat((bytes / Math.pow(k, i)).toFixed(1)) + ' ' + sizes[i]
}
</script>

<template>
  <div class="settings-view">
    <h2>{{ t('settings.title') }}</h2>

    <div v-if="loading" class="card"><p class="empty">{{ t('common.loading') }}</p></div>

    <template v-else>
      <!-- System Info -->
      <section class="section">
        <h3>{{ t('settings.system') }}</h3>
        <div class="card info-grid" v-if="config">
          <div class="info-row">
            <span class="info-label">{{ t('settings.mode') }}</span>
            <span class="info-value"><span class="status-badge status-running">{{ config.mode }}</span></span>
          </div>
          <div class="info-row">
            <span class="info-label">{{ t('settings.version') }}</span>
            <span class="info-value mono">{{ config.version }}</span>
          </div>
        </div>
      </section>

      <!-- Security Policy -->
      <section class="section" v-if="policy">
        <div class="section-header">
          <h3>{{ t('settings.security_policy') }}</h3>
          <button v-if="!editing" class="btn btn-sm btn-secondary" @click="startEdit">{{ t('common.edit') }}</button>
          <div v-else class="action-buttons">
            <button class="btn btn-sm btn-primary" @click="savePolicy" :disabled="saving">
              {{ saving ? t('common.saving') : t('common.save') }}
            </button>
            <button class="btn btn-sm btn-secondary" @click="cancelEdit">{{ t('common.cancel') }}</button>
          </div>
        </div>

        <p v-if="saveError" style="color: var(--color-error); font-size: 0.75rem; margin-bottom: 0.5rem">{{ saveError }}</p>

        <!-- Read mode -->
        <div v-if="!editing" class="card info-grid">
          <div class="info-row">
            <span class="info-label">{{ t('settings.autonomy_level') }}</span>
            <span class="info-value">
              <span :class="['status-badge', policy.autonomy_level === 'full' ? 'status-running' : 'status-completed']">
                {{ translateAutonomy(policy.autonomy_level) }}
              </span>
            </span>
          </div>
          <div class="info-row">
            <span class="info-label">{{ t('settings.max_calls_min') }}</span>
            <span class="info-value mono">{{ policy.max_calls_per_minute }}</span>
          </div>
          <div class="info-row">
            <span class="info-label">{{ t('settings.max_high_risk_min') }}</span>
            <span class="info-value mono">{{ policy.max_high_risk_per_minute }}</span>
          </div>
          <div class="info-row">
            <span class="info-label">{{ t('settings.network_access') }}</span>
            <span class="info-value">
              <span :class="['status-badge', policy.allow_network ? 'status-completed' : 'status-failed']">
                {{ policy.allow_network ? t('settings.allowed') : t('settings.blocked') }}
              </span>
            </span>
          </div>
          <div class="info-row">
            <span class="info-label">{{ t('settings.max_write_size') }}</span>
            <span class="info-value mono">{{ formatBytes(policy.max_write_size) }}</span>
          </div>
        </div>

        <!-- Edit mode -->
        <div v-else class="card edit-grid">
          <div class="edit-row">
            <label>{{ t('settings.autonomy_level') }}</label>
            <select v-model="editPolicy!.autonomy_level">
              <option value="supervised">{{ t('settings.autonomy_supervised') }}</option>
              <option value="semi">{{ t('settings.autonomy_semi') }}</option>
              <option value="full">{{ t('settings.autonomy_full') }}</option>
            </select>
          </div>
          <div class="edit-row">
            <label>{{ t('settings.max_calls_min') }}</label>
            <input type="number" v-model.number="editPolicy!.max_calls_per_minute" min="1" />
          </div>
          <div class="edit-row">
            <label>{{ t('settings.max_high_risk_min') }}</label>
            <input type="number" v-model.number="editPolicy!.max_high_risk_per_minute" min="0" />
          </div>
          <div class="edit-row">
            <label>{{ t('settings.network_access') }}</label>
            <label class="toggle">
              <input type="checkbox" v-model="editPolicy!.allow_network" />
              <span>{{ editPolicy!.allow_network ? t('settings.allowed') : t('settings.blocked') }}</span>
            </label>
          </div>
          <div class="edit-row">
            <label>{{ t('settings.max_write_bytes') }}</label>
            <input type="number" v-model.number="editPolicy!.max_write_size" min="0" />
          </div>
        </div>
      </section>

      <!-- Channels -->
      <section class="section" v-if="channels.length">
        <h3>{{ t('settings.channels') }} ({{ channels.length }})</h3>
        <div class="card">
          <div class="channel-grid">
            <span v-for="ch in channels" :key="ch" class="channel-chip">{{ ch }}</span>
          </div>
        </div>
      </section>

      <!-- Nodes -->
      <section class="section" v-if="nodes.length">
        <h3>{{ t('settings.nodes') }} ({{ nodes.length }})</h3>
        <div class="card">
          <table>
            <thead>
              <tr>
                <th>{{ t('settings.hostname') }}</th>
                <th>{{ t('common.status') }}</th>
                <th>{{ t('settings.agents_col') }}</th>
                <th>{{ t('settings.plugins') }}</th>
                <th>{{ t('settings.memory_col') }}</th>
                <th>{{ t('settings.last_heartbeat') }}</th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="node in nodes" :key="node.id">
                <td class="mono">{{ node.hostname }}</td>
                <td>
                  <span :class="['status-badge', `status-${node.status === 'online' ? 'completed' : node.status === 'draining' ? 'running' : 'failed'}`]">
                    {{ translateStatus(node.status) }}
                  </span>
                </td>
                <td>{{ node.capacity.running_agents }} / {{ node.capacity.max_concurrent }}</td>
                <td>{{ node.capacity.running_plugins }}</td>
                <td>{{ formatBytes(node.capacity.available_memory) }} / {{ formatBytes(node.capacity.total_memory) }}</td>
                <td>{{ new Date(node.last_heartbeat).toLocaleString() }}</td>
              </tr>
            </tbody>
          </table>
        </div>
      </section>
    </template>
  </div>
</template>

<style scoped>
.settings-view h2 { margin-bottom: 1.25rem; }

.section { margin-bottom: 1.5rem; }
.section h3 { margin-bottom: 0.75rem; font-size: 1rem; }
.section-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 0.75rem;
}
.section-header h3 { margin-bottom: 0; }

.info-grid { display: flex; flex-direction: column; gap: 0; }
.info-row {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 0.625rem 0;
  border-bottom: 1px solid var(--border-color);
}
.info-row:last-child { border-bottom: none; }
.info-label { color: var(--text-secondary); font-size: 0.875rem; }
.info-value { font-size: 0.875rem; }

.edit-grid { display: flex; flex-direction: column; gap: 0; }
.edit-row {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 0.625rem 0;
  border-bottom: 1px solid var(--border-color);
}
.edit-row:last-child { border-bottom: none; }
.edit-row label:first-child { color: var(--text-secondary); font-size: 0.875rem; }
.edit-row input, .edit-row select { max-width: 200px; }

.toggle {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  cursor: pointer;
  font-size: 0.875rem;
}

.channel-grid { display: flex; flex-wrap: wrap; gap: 0.5rem; }
.channel-chip {
  background: var(--bg-page);
  padding: 0.25rem 0.75rem;
  border-radius: 6px;
  font-size: 0.8125rem;
  color: var(--text-primary);
  border: 1px solid var(--border-color);
}
</style>
