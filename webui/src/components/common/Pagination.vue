<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'

const { t } = useI18n()

const props = withDefaults(defineProps<{
  total: number
  pageSize?: number
  currentPage: number
}>(), {
  pageSize: 20,
})

const emit = defineEmits<{
  'update:currentPage': [page: number]
}>()

const totalPages = computed(() => Math.max(1, Math.ceil(props.total / props.pageSize)))

const start = computed(() => props.total === 0 ? 0 : (props.currentPage - 1) * props.pageSize + 1)
const end = computed(() => Math.min(props.currentPage * props.pageSize, props.total))
</script>

<template>
  <div class="pagination">
    <span class="page-info">{{ t('pagination.showing', { start, end, total }) }}</span>
    <div class="page-controls">
      <button
        class="btn btn-sm btn-secondary"
        :disabled="currentPage <= 1"
        @click="emit('update:currentPage', currentPage - 1)"
      >
        {{ t('pagination.prev') }}
      </button>
      <span class="page-num">{{ currentPage }} / {{ totalPages }}</span>
      <button
        class="btn btn-sm btn-secondary"
        :disabled="currentPage >= totalPages"
        @click="emit('update:currentPage', currentPage + 1)"
      >
        {{ t('pagination.next') }}
      </button>
    </div>
  </div>
</template>

<style scoped>
.pagination {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 0.75rem 0.75rem 0;
  font-size: 0.8125rem;
  color: var(--text-secondary);
}
.page-controls {
  display: flex;
  align-items: center;
  gap: 0.5rem;
}
.page-num {
  min-width: 3rem;
  text-align: center;
}
</style>
