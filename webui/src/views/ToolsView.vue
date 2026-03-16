<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { useApi } from '../composables/useApi'
import { useI18n } from 'vue-i18n'
import type { ToolSchema } from '../types/api'

const { t } = useI18n()
const api = useApi()
const tools = ref<ToolSchema[]>([])
const loading = ref(false)
const expandedTool = ref<string | null>(null)

async function fetchTools() {
  loading.value = true
  try {
    tools.value = await api.get<ToolSchema[]>('/api/v1/tools')
  } catch { /* ignore */ }
  finally { loading.value = false }
}

function toggleTool(name: string) {
  expandedTool.value = expandedTool.value === name ? null : name
}

const groupedTools = computed(() => {
  const builtin: ToolSchema[] = []
  const native: ToolSchema[] = []
  const mcp: ToolSchema[] = []
  for (const tool of tools.value) {
    const params = tool.parameters as Record<string, any>
    if (params?._binding === 'mcp' || params?._mcp_server) {
      mcp.push(tool)
    } else if (params?._binding === 'native') {
      native.push(tool)
    } else {
      builtin.push(tool)
    }
  }
  return [
    { key: 'tools_page.group_builtin', tools: builtin },
    { key: 'tools_page.group_native', tools: native },
    { key: 'tools_page.group_mcp', tools: mcp },
  ]
})

onMounted(() => fetchTools())
</script>

<template>
  <div class="tools-view">
    <h2>{{ t('tools_page.title') }}</h2>
    <p class="subtitle">{{ t('tools_page.subtitle') }}</p>

    <div v-if="loading" class="card"><p class="empty">{{ t('common.loading') }}</p></div>

    <template v-else>
      <div v-for="group in groupedTools" :key="group.key">
        <template v-if="group.tools.length">
          <h3 class="group-label">{{ t(group.key) }} ({{ group.tools.length }})</h3>
          <div class="tool-list">
            <div v-for="tool in group.tools" :key="tool.name" class="card tool-card" @click="toggleTool(tool.name)">
              <div class="tool-header">
                <code class="tool-name">{{ tool.name }}</code>
                <span class="expand-icon">{{ expandedTool === tool.name ? '▲' : '▼' }}</span>
              </div>
              <p class="tool-desc">{{ tool.description }}</p>
              <div v-if="expandedTool === tool.name" class="tool-params">
                <h4>{{ t('tools_page.parameters') }}</h4>
                <pre>{{ JSON.stringify(tool.parameters, null, 2) }}</pre>
              </div>
            </div>
          </div>
        </template>
      </div>

      <div v-if="!tools.length" class="card"><p class="empty">{{ t('tools_page.no_tools') }}</p></div>
    </template>
  </div>
</template>

<style scoped>
.tools-view h2 { margin-bottom: 0.25rem; }
.subtitle { color: var(--text-secondary); font-size: 0.8125rem; margin-bottom: 1.25rem; }
.group-label { font-size: 0.875rem; font-weight: 600; margin: 1.25rem 0 0.5rem; color: var(--text-secondary); }
.tool-list { display: flex; flex-direction: column; gap: 0.5rem; }
.tool-card { cursor: pointer; transition: box-shadow 0.15s; }
.tool-card:hover { box-shadow: 0 1px 4px rgba(0,0,0,0.08); }
.tool-header { display: flex; justify-content: space-between; align-items: center; }
.tool-name { font-size: 0.9375rem; font-weight: 600; color: var(--color-primary); }
.tool-desc { color: var(--text-secondary); font-size: 0.8125rem; margin-top: 0.25rem; }
.tool-params { margin-top: 0.75rem; }
.tool-params h4 { font-size: 0.8125rem; color: var(--text-secondary); margin-bottom: 0.375rem; }
.tool-params pre {
  background: var(--bg-page);
  padding: 0.75rem;
  border-radius: 6px;
  font-size: 0.75rem;
  overflow-x: auto;
  max-height: 300px;
}
.expand-icon { color: var(--text-secondary); font-size: 0.75rem; }
.empty { color: var(--text-secondary); padding: 2rem 0; text-align: center; }
</style>
