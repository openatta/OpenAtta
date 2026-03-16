<script setup lang="ts">
import { ref } from 'vue'
import { useMemoryStore } from '../stores/memory'
import { useI18n } from 'vue-i18n'

const { t } = useI18n()
const store = useMemoryStore()
const query = ref('')

async function doSearch() {
  if (!query.value.trim()) return
  await store.search(query.value)
}

async function removeEntry(id: string) {
  if (!confirm(t('common.confirm_delete'))) return
  await store.remove(id)
}

function formatDate(ts: string): string {
  try {
    return new Date(ts).toLocaleDateString()
  } catch {
    return ts
  }
}
</script>

<template>
  <div class="memory-view">
    <div class="header-row">
      <h2>{{ t('memory.title') }}</h2>
    </div>

    <div class="filter-bar">
      <input
        v-model="query"
        type="text"
        :placeholder="t('memory.search_placeholder')"
        class="search-input"
        @keyup.enter="doSearch"
      />
      <button class="btn btn-primary" @click="doSearch" :disabled="!query.trim() || store.loading">
        {{ store.loading ? t('common.searching') : t('common.search') }}
      </button>
    </div>

    <div class="card" v-if="store.entries.length">
      <div v-for="entry in store.entries" :key="entry.id" class="memory-entry">
        <div class="entry-content">{{ entry.content }}</div>
        <div class="entry-meta">
          <span v-if="entry.score != null" class="chip">score: {{ entry.score.toFixed(2) }}</span>
          <span class="meta-date">{{ formatDate(entry.created_at) }}</span>
          <span v-if="entry.metadata && Object.keys(entry.metadata).length" class="meta-tags">
            <span v-for="(v, k) in entry.metadata" :key="String(k)" class="chip">{{ k }}: {{ v }}</span>
          </span>
          <button class="btn btn-sm btn-danger" @click="removeEntry(entry.id)">{{ t('common.delete') }}</button>
        </div>
      </div>
      <div class="entry-count">{{ store.entries.length }} {{ t('memory.results') }}</div>
    </div>

    <div v-else-if="!store.loading" class="card">
      <p class="empty">{{ t('memory.empty_hint') }}</p>
    </div>
  </div>
</template>

<style scoped>
.memory-view h2 { margin-bottom: 0; }
.memory-entry {
  padding: 0.75rem 0;
  border-bottom: 1px solid var(--border-color);
}
.memory-entry:last-child { border-bottom: none; }
.entry-content {
  font-size: 0.9375rem;
  line-height: 1.5;
  margin-bottom: 0.5rem;
}
.entry-meta {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  flex-wrap: wrap;
}
.meta-date {
  font-size: 0.75rem;
  color: var(--text-secondary);
}
.meta-tags {
  display: flex;
  gap: 0.25rem;
}
.entry-count {
  padding-top: 0.75rem;
  font-size: 0.75rem;
  color: var(--text-secondary);
  text-align: right;
}
</style>
