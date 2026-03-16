<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { useSkillStore } from '../stores/skill'
import { useI18n } from 'vue-i18n'
import Pagination from '../components/common/Pagination.vue'
import type { SkillDef } from '../types/api'

const { t } = useI18n()
const skillStore = useSkillStore()
const showImport = ref(false)
const importText = ref('')
const importError = ref('')
const selectedSkill = ref<SkillDef | null>(null)
const currentPage = ref(1)
const pageSize = ref(20)

const paginatedSkills = computed(() => {
  const start = (currentPage.value - 1) * pageSize.value
  return skillStore.skills.slice(start, start + pageSize.value)
})

function riskClass(level: string) {
  return level === 'high' ? 'risk-high' : level === 'medium' ? 'risk-medium' : 'risk-low'
}

function viewSkill(skill: SkillDef) {
  selectedSkill.value = selectedSkill.value?.id === skill.id ? null : skill
}

async function importSkill() {
  importError.value = ''
  try {
    let parsed: any
    try {
      parsed = JSON.parse(importText.value)
    } catch {
      importError.value = t('error.invalid_json')
      return
    }
    await skillStore.createSkill(parsed)
    importText.value = ''
    showImport.value = false
    currentPage.value = 1
    await skillStore.fetchSkills()
  } catch (e: any) {
    importError.value = e.message || t('error.failed_import_skill')
  }
}

async function deleteSkill(id: string) {
  if (!confirm(t('confirm.delete_skill', { id }))) return
  try {
    await skillStore.deleteSkill(id)
    currentPage.value = 1
    await skillStore.fetchSkills()
  } catch {
    // Error toast shown by global onResponseError handler
  }
}

onMounted(() => skillStore.fetchSkills())
</script>

<template>
  <div class="skills-view">
    <div class="header-row">
      <h2>{{ t('skills.title') }}</h2>
      <button class="btn btn-primary" @click="showImport = !showImport">
        {{ showImport ? t('common.cancel') : t('skills.import_skill') }}
      </button>
    </div>

    <!-- Import panel -->
    <div v-if="showImport" class="card import-panel">
      <h3>{{ t('skills.import_title') }}</h3>
      <p class="hint">{{ t('skills.import_hint') }}</p>
      <textarea v-model="importText" rows="8" placeholder='{"id": "my-skill", "version": "1.0", ...}'></textarea>
      <p v-if="importError" class="error-text">{{ importError }}</p>
      <button class="btn btn-primary" @click="importSkill" :disabled="!importText.trim()">{{ t('skills.import') }}</button>
    </div>

    <div v-if="skillStore.loading" class="card"><p class="empty">{{ t('common.loading') }}</p></div>

    <div v-else-if="skillStore.skills.length" class="skill-list">
      <div v-for="skill in paginatedSkills" :key="skill.id" class="card skill-card">
        <div class="skill-header">
          <div class="skill-title">
            <strong>{{ skill.name || skill.id }}</strong>
            <span :class="['status-badge', riskClass(skill.risk_level)]">{{ skill.risk_level }}</span>
            <span :class="['source-badge', skill.source === 'imported' ? 'source-imported' : 'source-builtin']">
              {{ skill.source ? skill.source : t('common.source_builtin') }}
            </span>
          </div>
          <div class="skill-actions">
            <button class="btn btn-sm" @click="viewSkill(skill)">
              {{ selectedSkill?.id === skill.id ? t('common.hide') : t('common.view') }}
            </button>
            <button v-if="skill.source === 'imported'" class="btn btn-sm btn-danger" @click="deleteSkill(skill.id)">
              {{ t('common.delete') }}
            </button>
          </div>
        </div>
        <p v-if="skill.description" class="skill-desc">{{ skill.description }}</p>
        <div class="skill-meta">
          <span v-if="skill.version" class="meta-item">v{{ skill.version }}</span>
          <span v-if="skill.author" class="meta-item">{{ skill.author }}</span>
          <span v-if="skill.requires_approval" class="meta-item approval">{{ t('skills.requires_approval') }}</span>
        </div>
        <div v-if="skill.tags.length" class="skill-tags">
          <span v-for="tag in skill.tags" :key="tag" class="tag">{{ tag }}</span>
        </div>
        <div v-if="skill.tools.length" class="skill-tools">
          <span class="meta-label">{{ t('skills.tools_label') }}</span>
          <code v-for="tl in skill.tools" :key="tl" class="tool-ref">{{ tl }}</code>
        </div>

        <!-- Detail view -->
        <div v-if="selectedSkill?.id === skill.id" class="skill-detail">
          <h4>{{ t('skills.system_prompt') }}</h4>
          <pre>{{ skill.system_prompt }}</pre>
        </div>
      </div>
    </div>
    <Pagination
      v-if="skillStore.skills.length"
      :total="skillStore.skills.length"
      :page-size="pageSize"
      v-model:current-page="currentPage"
    />
    <div v-else class="card"><p class="empty">{{ t('skills.no_skills') }}</p></div>
  </div>
</template>

<style scoped>
.skills-view h2 { margin-bottom: 0; }
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
.skill-list { display: flex; flex-direction: column; gap: 0.5rem; }
.skill-header { display: flex; justify-content: space-between; align-items: center; }
.skill-title { display: flex; align-items: center; gap: 0.5rem; }
.skill-actions { display: flex; gap: 0.375rem; }
.skill-desc { color: var(--text-secondary); font-size: 0.8125rem; margin-top: 0.375rem; }
.skill-meta { display: flex; gap: 0.75rem; margin-top: 0.5rem; font-size: 0.75rem; }
.meta-item { color: var(--text-secondary); }
.meta-label { color: var(--text-secondary); font-size: 0.75rem; margin-right: 0.25rem; }
.approval { color: var(--color-warning); font-weight: 500; }
.skill-tags { display: flex; gap: 0.375rem; margin-top: 0.5rem; flex-wrap: wrap; }
.tag { background: var(--bg-page); padding: 0.125rem 0.5rem; border-radius: 4px; font-size: 0.6875rem; color: var(--text-secondary); }
.skill-tools { margin-top: 0.5rem; display: flex; align-items: center; gap: 0.375rem; flex-wrap: wrap; }
.tool-ref { background: #eef2ff; color: var(--color-primary); padding: 0.0625rem 0.375rem; border-radius: 4px; font-size: 0.6875rem; }
.source-badge { font-size: 0.6875rem; padding: 0.0625rem 0.375rem; border-radius: 4px; font-weight: 500; }
.source-builtin { background: #dbeafe; color: #1d4ed8; }
.source-imported { background: #fef3c7; color: #92400e; }
.risk-high { background: #fee2e2; color: #b91c1c; }
.risk-medium { background: #fef3c7; color: #92400e; }
.risk-low { background: #dcfce7; color: #15803d; }
.skill-detail { margin-top: 0.75rem; border-top: 1px solid var(--border-color); padding-top: 0.75rem; }
.skill-detail h4 { font-size: 0.8125rem; color: var(--text-secondary); margin-bottom: 0.375rem; }
.skill-detail pre {
  background: var(--bg-page);
  padding: 0.75rem;
  border-radius: 6px;
  font-size: 0.75rem;
  overflow-x: auto;
  max-height: 300px;
  white-space: pre-wrap;
}
.btn-sm { padding: 0.25rem 0.5rem; font-size: 0.75rem; }
.btn-danger { background: #ef4444; color: #fff; border: none; border-radius: 4px; cursor: pointer; }
.btn-danger:hover { background: #dc2626; }
.empty { color: var(--text-secondary); padding: 2rem 0; text-align: center; }
</style>
