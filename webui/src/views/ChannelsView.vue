<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useChannelStore } from '../stores/channel'
import { useI18n } from 'vue-i18n'
import type { ChannelInfo } from '../types/api'

const { t } = useI18n()
const store = useChannelStore()
const selectedChannel = ref<string | null>(null)
const healthResult = ref<any>(null)
const healthLoading = ref(false)
const showAdd = ref(false)
const addForm = ref({ name: '', channel_type: 'webhook', config: '{}' })
const addError = ref('')

const channelTypes = [
  { groupKey: 'channels.messaging', items: ['slack', 'discord', 'telegram', 'whatsapp', 'signal', 'qq', 'imessage'] },
  { groupKey: 'channels.enterprise', items: ['dingtalk', 'lark', 'mattermost', 'nextcloud'] },
  { groupKey: 'channels.open_protocol', items: ['irc', 'matrix', 'nostr', 'mqtt'] },
  { groupKey: 'channels.notification', items: ['email', 'webhook'] },
  { groupKey: 'channels.local', items: ['terminal'] },
]

onMounted(() => store.fetchChannels())

async function selectChannel(name: string) {
  if (selectedChannel.value === name) {
    selectedChannel.value = null
    return
  }
  selectedChannel.value = name
  await checkHealth(name)
}

async function checkHealth(name: string) {
  healthLoading.value = true
  healthResult.value = null
  try {
    const resp = await store.checkHealth(name)
    healthResult.value = resp.data
  } catch (e: any) {
    healthResult.value = { status: 'error', error: e.message || t('error.health_check_failed') }
  }
  healthLoading.value = false
}

async function addChannel() {
  addError.value = ''
  try {
    const config = JSON.parse(addForm.value.config)
    await store.addChannel({
      name: addForm.value.name || addForm.value.channel_type,
      channel_type: addForm.value.channel_type,
      ...config,
    })
    showAdd.value = false
    addForm.value = { name: '', channel_type: 'webhook', config: '{}' }
    await store.fetchChannels()
  } catch (e: any) {
    addError.value = e.message || t('error.failed_add_channel')
  }
}

async function removeChannel(name: string) {
  if (!confirm(t('confirm.remove_channel', { name }))) return
  await store.removeChannel(name)
  if (selectedChannel.value === name) selectedChannel.value = null
  await store.fetchChannels()
}
</script>

<template>
  <div class="channels-view">
    <div class="header-row">
      <h2>{{ t('channels.title') }}</h2>
      <button class="btn btn-primary" @click="showAdd = !showAdd">
        {{ showAdd ? t('common.cancel') : t('channels.add_channel') }}
      </button>
    </div>

    <!-- Add channel form -->
    <div v-if="showAdd" class="card" style="margin-bottom: 1rem">
      <h3 style="font-size: 0.875rem; margin-bottom: 0.75rem">{{ t('channels.add_title') }}</h3>
      <div class="form-grid">
        <div class="form-field">
          <label>{{ t('channels.name') }}</label>
          <input v-model="addForm.name" placeholder="my-slack" />
        </div>
        <div class="form-field">
          <label>{{ t('channels.type') }}</label>
          <select v-model="addForm.channel_type">
            <optgroup v-for="g in channelTypes" :key="g.groupKey" :label="t(g.groupKey)">
              <option v-for="tp in g.items" :key="tp" :value="tp">{{ tp }}</option>
            </optgroup>
          </select>
        </div>
        <div class="form-field" style="grid-column: 1 / -1">
          <label>{{ t('channels.config_label') }}</label>
          <textarea v-model="addForm.config" rows="4" class="mono" placeholder='{"token": "xoxb-..."}'></textarea>
        </div>
      </div>
      <p v-if="addError" style="color: var(--color-error); font-size: 0.75rem; margin-bottom: 0.5rem">{{ addError }}</p>
      <button class="btn btn-primary btn-sm" @click="addChannel">{{ t('common.add') }}</button>
    </div>

    <!-- Channel grid -->
    <div class="channel-grid" v-if="store.channels.length">
      <div
        v-for="ch in store.channels"
        :key="ch.name"
        :class="['channel-card', { selected: selectedChannel === ch.name }]"
        @click="selectChannel(ch.name)"
      >
        <div class="ch-header">
          <span :class="['ch-dot', ch.healthy ? 'healthy' : 'unhealthy']"></span>
          <span class="ch-name">{{ ch.name }}</span>
        </div>
        <div class="ch-status">{{ ch.healthy ? t('common.healthy') : t('common.unhealthy') }}</div>
      </div>
    </div>
    <div v-else class="card">
      <p class="empty">{{ t('channels.no_channels') }}</p>
    </div>

    <!-- Channel detail -->
    <div v-if="selectedChannel" class="card" style="margin-top: 1rem">
      <div style="display: flex; justify-content: space-between; align-items: center; margin-bottom: 0.75rem">
        <h3 style="font-size: 0.9375rem">{{ selectedChannel }}</h3>
        <div class="action-buttons">
          <button class="btn btn-sm btn-secondary" @click="checkHealth(selectedChannel!)" :disabled="healthLoading">
            {{ healthLoading ? t('channels.checking') : t('channels.test_connection') }}
          </button>
          <button class="btn btn-sm btn-danger" @click="removeChannel(selectedChannel!)">{{ t('common.remove') }}</button>
        </div>
      </div>

      <div v-if="healthResult" class="health-result">
        <div class="detail-row">
          <span class="detail-label">{{ t('common.status') }}</span>
          <span :class="['status-badge', healthResult.status === 'healthy' ? 'status-completed' : 'status-failed']">
            {{ healthResult.status }}
          </span>
        </div>
        <div v-if="healthResult.error" class="detail-row">
          <span class="detail-label">{{ t('common.error') }}</span>
          <span style="color: var(--color-error)">{{ healthResult.error }}</span>
        </div>
      </div>
      <div v-else-if="healthLoading" class="empty" style="padding: 1rem 0">{{ t('channels.checking') }}</div>
    </div>

    <!-- Available channel types -->
    <div class="card" style="margin-top: 1rem">
      <h3 style="font-size: 0.9375rem; margin-bottom: 0.75rem">{{ t('channels.available_types') }}</h3>
      <div v-for="g in channelTypes" :key="g.groupKey" style="margin-bottom: 0.75rem">
        <div style="font-size: 0.75rem; color: var(--text-secondary); margin-bottom: 0.375rem; font-weight: 600">{{ t(g.groupKey) }}</div>
        <div style="display: flex; flex-wrap: wrap; gap: 0.375rem">
          <span v-for="tp in g.items" :key="tp" class="chip">{{ tp }}</span>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.channels-view h2 { margin-bottom: 0; }
.channel-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
  gap: 0.75rem;
}
.channel-card {
  background: var(--bg-surface);
  border: 1px solid var(--border-color);
  border-radius: 8px;
  padding: 1rem;
  cursor: pointer;
  transition: border-color 0.15s, box-shadow 0.15s;
}
.channel-card:hover {
  border-color: var(--color-primary);
}
.channel-card.selected {
  border-color: var(--color-primary);
  box-shadow: 0 0 0 1px var(--color-primary);
}
.ch-header {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  margin-bottom: 0.375rem;
}
.ch-dot {
  width: 8px; height: 8px; border-radius: 50%;
}
.ch-dot.healthy { background: var(--color-success); }
.ch-dot.unhealthy { background: var(--color-error); }
.ch-name { font-weight: 600; font-size: 0.9375rem; }
.ch-status { font-size: 0.75rem; color: var(--text-secondary); }
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
.form-field input, .form-field select, .form-field textarea {
  width: 100%;
}
.health-result {
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
}
.detail-row {
  display: flex;
  align-items: center;
  gap: 0.75rem;
  font-size: 0.875rem;
}
.detail-label {
  color: var(--text-secondary);
  min-width: 80px;
}
</style>
