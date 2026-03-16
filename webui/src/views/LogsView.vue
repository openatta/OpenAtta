<script setup lang="ts">
import { ref, watch, nextTick, onMounted, onUnmounted } from 'vue'
import { useLogStore } from '../stores/log'
import { useI18n } from 'vue-i18n'

const { t } = useI18n()
const store = useLogStore()
const container = ref<HTMLElement>()
const autoScroll = ref(true)

const levels = ['trace', 'debug', 'info', 'warn', 'error'] as const

const levelColors: Record<string, string> = {
  trace: 'var(--text-secondary)',
  debug: '#60a5fa',
  info: 'var(--color-success)',
  warn: 'var(--color-warning)',
  error: 'var(--color-error)',
}

onMounted(() => store.connect())
onUnmounted(() => store.disconnect())

watch(() => store.filtered.length, () => {
  if (autoScroll.value) {
    nextTick(() => {
      if (container.value) {
        container.value.scrollTop = container.value.scrollHeight
      }
    })
  }
})

function onScroll() {
  if (!container.value) return
  const { scrollTop, scrollHeight, clientHeight } = container.value
  autoScroll.value = scrollHeight - scrollTop - clientHeight < 40
}

function scrollToBottom() {
  if (container.value) {
    container.value.scrollTop = container.value.scrollHeight
    autoScroll.value = true
  }
}

function formatTime(ts: string): string {
  try {
    return new Date(ts).toLocaleTimeString()
  } catch {
    return ts
  }
}
</script>

<template>
  <div class="logs-view">
    <div class="header-row">
      <h2>{{ t('logs.title') }}</h2>
      <div class="action-buttons">
        <span :class="['conn-dot', store.connected ? 'on' : 'off']"></span>
        <span class="conn-label">{{ store.connected ? t('common.connected') : t('common.disconnected') }}</span>
        <button class="btn btn-sm btn-secondary" @click="store.paused = !store.paused">
          {{ store.paused ? ('▶ ' + t('logs.resume')) : ('⏸ ' + t('logs.pause')) }}
        </button>
        <button class="btn btn-sm btn-secondary" @click="store.clear()">{{ t('logs.clear') }}</button>
      </div>
    </div>

    <!-- Filters -->
    <div class="filter-bar">
      <button
        v-for="lvl in levels"
        :key="lvl"
        :class="['chip', store.levelFilter.has(lvl) ? 'active' : '']"
        @click="store.toggleLevel(lvl)"
        :style="store.levelFilter.has(lvl) ? { background: levelColors[lvl], borderColor: levelColors[lvl], color: '#fff' } : {}"
      >{{ lvl }}</button>
      <input
        v-model="store.textFilter"
        type="text"
        :placeholder="t('logs.filter_placeholder')"
        class="search-input"
      />
    </div>

    <!-- Log entries -->
    <div class="log-container" ref="container" @scroll="onScroll">
      <div v-if="!store.filtered.length" class="empty">
        {{ store.connected ? t('logs.waiting') : t('logs.not_connected') }}
      </div>
      <div v-for="(entry, i) in store.filtered" :key="i" class="log-entry">
        <span class="log-time">{{ formatTime(entry.timestamp) }}</span>
        <span class="log-level" :style="{ color: levelColors[entry.level] }">{{ entry.level.toUpperCase().padEnd(5) }}</span>
        <span class="log-target">{{ entry.target }}</span>
        <span class="log-msg">{{ entry.message }}</span>
      </div>
    </div>

    <!-- Scroll to bottom -->
    <button
      v-if="!autoScroll && store.filtered.length > 0"
      class="scroll-btn"
      @click="scrollToBottom"
    >↓ {{ t('logs.scroll_bottom') }}</button>

    <div class="log-footer">
      {{ store.filtered.length }} / {{ store.entries.length }} {{ t('logs.entries') }}
    </div>
  </div>
</template>

<style scoped>
.logs-view { display: flex; flex-direction: column; height: 100%; }
.logs-view h2 { margin-bottom: 0; }
.conn-dot {
  width: 8px; height: 8px; border-radius: 50%; display: inline-block;
}
.conn-dot.on { background: var(--color-success); }
.conn-dot.off { background: var(--color-error); }
.conn-label { font-size: 0.75rem; color: var(--text-secondary); }

.log-container {
  flex: 1;
  overflow-y: auto;
  background: var(--bg-code, #1e1e2e);
  border: 1px solid var(--border-color);
  border-radius: 8px;
  padding: 0.5rem;
  font-family: 'SF Mono', 'Fira Code', monospace;
  font-size: 0.8125rem;
  line-height: 1.6;
  min-height: 300px;
}

.log-entry {
  display: flex;
  gap: 0.75rem;
  white-space: nowrap;
  color: var(--text-code, #cdd6f4);
}

.log-time {
  color: var(--text-secondary);
  min-width: 80px;
}

.log-level {
  min-width: 45px;
  font-weight: 600;
}

.log-target {
  color: #89b4fa;
  min-width: 120px;
  max-width: 200px;
  overflow: hidden;
  text-overflow: ellipsis;
}

.log-msg {
  flex: 1;
  white-space: pre-wrap;
  word-break: break-all;
}

.scroll-btn {
  position: fixed;
  bottom: 4rem;
  right: 2rem;
  background: var(--color-primary);
  color: white;
  border: none;
  border-radius: 999px;
  padding: 0.375rem 1rem;
  font-size: 0.75rem;
  cursor: pointer;
  box-shadow: var(--shadow-md);
  z-index: 10;
}

.log-footer {
  padding: 0.5rem 0;
  font-size: 0.75rem;
  color: var(--text-secondary);
  text-align: right;
}
</style>
