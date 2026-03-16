<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useMcpStore } from '../stores/mcp'
import { useI18n } from 'vue-i18n'
import type { McpServerInfo } from '../types/api'

const { t } = useI18n()
const mcpStore = useMcpStore()
const showAdd = ref(false)
const addName = ref('')
const addTransport = ref<'stdio' | 'sse'>('stdio')
const addCommand = ref('')
const addUrl = ref('')
const addArgs = ref('')
const addError = ref('')
const serverDetails = ref<Record<string, McpServerInfo>>({})

async function loadServerDetails(name: string) {
  if (serverDetails.value[name]) {
    delete serverDetails.value[name]
    return
  }
  try {
    const info = await mcpStore.getMcpServer(name)
    serverDetails.value[name] = info
  } catch (e: any) {
    serverDetails.value[name] = { name, tools: [] }
  }
}

async function addServer() {
  addError.value = ''
  if (!addName.value.trim()) {
    addError.value = t('error.name_required')
    return
  }

  const config: Record<string, any> = {
    name: addName.value.trim(),
    transport: addTransport.value,
  }

  if (addTransport.value === 'stdio') {
    if (!addCommand.value.trim()) { addError.value = t('error.command_required'); return }
    config.command = addCommand.value.trim()
    config.args = addArgs.value.trim() ? addArgs.value.trim().split(/\s+/) : []
  } else {
    if (!addUrl.value.trim()) { addError.value = t('error.url_required'); return }
    config.url = addUrl.value.trim()
  }

  try {
    await mcpStore.registerMcp(config as any)
    addName.value = ''
    addCommand.value = ''
    addUrl.value = ''
    addArgs.value = ''
    showAdd.value = false
    await mcpStore.fetchMcpServers()
  } catch (e: any) {
    addError.value = e.message || t('error.failed_register_mcp')
  }
}

async function removeServer(name: string) {
  if (!confirm(t('confirm.remove_mcp_server', { name }))) return
  try {
    await mcpStore.unregisterMcp(name)
    delete serverDetails.value[name]
    await mcpStore.fetchMcpServers()
  } catch (e: any) {
    alert(e.message || t('error.failed_remove_mcp'))
  }
}

onMounted(() => mcpStore.fetchMcpServers())
</script>

<template>
  <div class="mcp-view">
    <div class="header-row">
      <h2>{{ t('mcp.title') }}</h2>
      <button class="btn btn-primary" @click="showAdd = !showAdd">
        {{ showAdd ? t('common.cancel') : t('mcp.add_server') }}
      </button>
    </div>

    <!-- Add panel -->
    <div v-if="showAdd" class="card add-panel">
      <h3>{{ t('mcp.register_title') }}</h3>
      <div class="form-group">
        <label>{{ t('mcp.name') }}</label>
        <input v-model="addName" placeholder="my-server" />
      </div>
      <div class="form-group">
        <label>{{ t('mcp.transport') }}</label>
        <select v-model="addTransport">
          <option value="stdio">{{ t('mcp.transport_stdio') }}</option>
          <option value="sse">{{ t('mcp.transport_sse') }}</option>
        </select>
      </div>
      <div v-if="addTransport === 'stdio'" class="form-group">
        <label>{{ t('mcp.command') }}</label>
        <input v-model="addCommand" placeholder="npx -y @modelcontextprotocol/server-xxx" />
      </div>
      <div v-if="addTransport === 'stdio'" class="form-group">
        <label>{{ t('mcp.args') }}</label>
        <input v-model="addArgs" placeholder="--flag value" />
      </div>
      <div v-if="addTransport === 'sse'" class="form-group">
        <label>{{ t('mcp.url') }}</label>
        <input v-model="addUrl" placeholder="http://localhost:8080/sse" />
      </div>
      <p v-if="addError" class="error-text">{{ addError }}</p>
      <button class="btn btn-primary" @click="addServer">{{ t('mcp.register') }}</button>
    </div>

    <div v-if="mcpStore.loading" class="card"><p class="empty">{{ t('common.loading') }}</p></div>

    <div v-else-if="mcpStore.servers.length" class="server-list">
      <div v-for="name in mcpStore.servers" :key="name" class="card server-card">
        <div class="server-header">
          <strong>{{ name }}</strong>
          <div class="server-actions">
            <button class="btn btn-sm" @click="loadServerDetails(name)">
              {{ serverDetails[name] ? t('common.hide') : t('common.details') }}
            </button>
            <button class="btn btn-sm btn-danger" @click="removeServer(name)">{{ t('common.remove') }}</button>
          </div>
        </div>

        <div v-if="serverDetails[name]" class="server-detail">
          <template v-if="serverDetails[name].tools?.length">
            <h4>{{ t('mcp.tools_count', { count: serverDetails[name].tools!.length }) }}</h4>
            <div v-for="tool in serverDetails[name].tools" :key="tool.name" class="mcp-tool">
              <code>{{ tool.name }}</code>
              <span class="tool-desc">{{ tool.description }}</span>
            </div>
          </template>
          <p v-else class="empty-small">{{ t('mcp.no_tools') }}</p>
        </div>
      </div>
    </div>
    <div v-else class="card"><p class="empty">{{ t('mcp.no_servers') }}</p></div>
  </div>
</template>

<style scoped>
.mcp-view h2 { margin-bottom: 0; }
.header-row { display: flex; justify-content: space-between; align-items: center; margin-bottom: 1.25rem; }
.add-panel { margin-bottom: 1rem; }
.add-panel h3 { font-size: 0.875rem; margin-bottom: 0.75rem; }
.form-group { margin-bottom: 0.625rem; }
.form-group label { display: block; font-size: 0.75rem; font-weight: 500; color: var(--text-secondary); margin-bottom: 0.25rem; }
.form-group input, .form-group select {
  width: 100%;
  padding: 0.375rem 0.5rem;
  font-size: 0.8125rem;
  border: 1px solid var(--border-color);
  border-radius: 6px;
  background: var(--bg-page);
  color: var(--text-primary);
}
.error-text { color: #ef4444; font-size: 0.75rem; margin-bottom: 0.5rem; }
.server-list { display: flex; flex-direction: column; gap: 0.5rem; }
.server-header { display: flex; justify-content: space-between; align-items: center; }
.server-actions { display: flex; gap: 0.375rem; }
.server-detail { margin-top: 0.75rem; border-top: 1px solid var(--border-color); padding-top: 0.75rem; }
.server-detail h4 { font-size: 0.8125rem; color: var(--text-secondary); margin-bottom: 0.5rem; }
.mcp-tool { display: flex; align-items: baseline; gap: 0.5rem; padding: 0.25rem 0; }
.mcp-tool code { font-size: 0.8125rem; font-weight: 600; color: var(--color-primary); }
.tool-desc { font-size: 0.75rem; color: var(--text-secondary); }
.btn-sm { padding: 0.25rem 0.5rem; font-size: 0.75rem; }
.btn-danger { background: #ef4444; color: #fff; border: none; border-radius: 4px; cursor: pointer; }
.btn-danger:hover { background: #dc2626; }
.empty { color: var(--text-secondary); padding: 2rem 0; text-align: center; }
.empty-small { color: var(--text-secondary); font-size: 0.75rem; padding: 0.5rem 0; }
</style>
