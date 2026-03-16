<script setup lang="ts">
import { ref } from 'vue'
import { useRouter, useRoute } from 'vue-router'
import { useI18n } from 'vue-i18n'

const router = useRouter()
const route = useRoute()
const { t } = useI18n()

interface NavGroup {
  labelKey: string
  items: { labelKey: string; path: string }[]
}

const navGroups: NavGroup[] = [
  {
    labelKey: 'nav.core',
    items: [
      { labelKey: 'nav.dashboard', path: '/' },
      { labelKey: 'nav.chat', path: '/chat' },
      { labelKey: 'nav.tasks', path: '/tasks' },
      { labelKey: 'nav.agents', path: '/agents' },
    ],
  },
  {
    labelKey: 'nav.orchestration',
    items: [
      { labelKey: 'nav.flows', path: '/flows' },
      { labelKey: 'nav.approvals', path: '/approvals' },
      { labelKey: 'nav.skills', path: '/skills' },
      { labelKey: 'nav.tools', path: '/tools' },
      { labelKey: 'nav.mcp', path: '/mcp' },
      { labelKey: 'nav.cron', path: '/cron' },
    ],
  },
  {
    labelKey: 'nav.operations',
    items: [
      { labelKey: 'nav.channels', path: '/channels' },
      { labelKey: 'nav.usage', path: '/usage' },
      { labelKey: 'nav.logs', path: '/logs' },
      { labelKey: 'nav.memory', path: '/memory' },
    ],
  },
  {
    labelKey: 'nav.system',
    items: [
      { labelKey: 'nav.settings', path: '/settings' },
      { labelKey: 'nav.audit', path: '/audit' },
      { labelKey: 'nav.diagnostics', path: '/diagnostics' },
    ],
  },
]

const collapsed = ref<Record<string, boolean>>({})

function toggleGroup(key: string) {
  collapsed.value[key] = !collapsed.value[key]
}

function isActive(path: string) {
  return route.path === path || (path !== '/' && route.path.startsWith(path))
}
</script>

<template>
  <aside class="sidebar">
    <div class="sidebar-header">
      <h1 class="logo">AttaOS</h1>
    </div>
    <nav class="sidebar-nav">
      <div v-for="group in navGroups" :key="group.labelKey" class="nav-group">
        <div class="group-label" @click="toggleGroup(group.labelKey)">
          <span>{{ t(group.labelKey) }}</span>
          <span class="chevron" :class="{ open: !collapsed[group.labelKey] }">›</span>
        </div>
        <div v-show="!collapsed[group.labelKey]" class="group-items">
          <a
            v-for="item in group.items"
            :key="item.path"
            :class="['nav-item', { active: isActive(item.path) }]"
            @click="router.push(item.path)"
          >
            {{ t(item.labelKey) }}
          </a>
        </div>
      </div>
    </nav>
  </aside>
</template>

<style scoped>
.sidebar {
  width: 220px;
  min-width: 220px;
  background: var(--bg-sidebar, #1a1a2e);
  color: var(--text-sidebar, #e0e0e0);
  display: flex;
  flex-direction: column;
  overflow-y: auto;
}
.sidebar-header {
  padding: 1.25rem 1rem;
  border-bottom: 1px solid rgba(255,255,255,0.1);
}
.logo {
  font-size: 1.25rem;
  font-weight: 700;
  margin: 0;
}
.sidebar-nav {
  padding: 0.5rem 0;
  flex: 1;
}
.nav-group {
  margin-bottom: 0.25rem;
}
.group-label {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 0.375rem 1rem;
  font-size: 0.6875rem;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  color: rgba(255,255,255,0.4);
  cursor: pointer;
  user-select: none;
}
.group-label:hover {
  color: rgba(255,255,255,0.6);
}
.chevron {
  font-size: 0.875rem;
  transition: transform 0.15s;
  transform: rotate(90deg);
}
.chevron.open {
  transform: rotate(270deg);
}
.group-items {
  padding-bottom: 0.25rem;
}
.nav-item {
  display: block;
  padding: 0.5rem 1rem 0.5rem 1.5rem;
  cursor: pointer;
  color: inherit;
  text-decoration: none;
  border-left: 3px solid transparent;
  transition: background 0.15s;
  font-size: 0.875rem;
}
.nav-item:hover {
  background: rgba(255,255,255,0.05);
}
.nav-item.active {
  background: rgba(255,255,255,0.1);
  border-left-color: var(--color-primary, #6366f1);
}
</style>
