<script setup lang="ts">
import { ref } from 'vue'
import { useApi } from '../../composables/useApi'
import { useI18n } from 'vue-i18n'

const { t } = useI18n()

const props = defineProps<{
  approvalId: string
}>()

const emit = defineEmits<{
  (e: 'decided'): void
}>()

const api = useApi()
const comment = ref('')
const processing = ref(false)
const error = ref<string | null>(null)

async function decide(action: 'approve' | 'deny' | 'request-changes') {
  processing.value = true
  error.value = null
  try {
    await api.post(`/api/v1/approvals/${props.approvalId}/${action}`, {
      comment: comment.value || undefined,
    })
    emit('decided')
  } catch (e: any) {
    error.value = e.message || t('error.failed_action', { action })
  } finally {
    processing.value = false
  }
}
</script>

<template>
  <div class="approval-actions">
    <h4>{{ t('approval.title') }}</h4>

    <div class="comment-field">
      <textarea
        v-model="comment"
        class="comment-input"
        :placeholder="t('approval.comment_placeholder')"
        rows="2"
      />
    </div>

    <div v-if="error" class="error-msg">{{ error }}</div>

    <div class="action-buttons">
      <button
        class="btn btn-approve"
        :disabled="processing"
        @click="decide('approve')"
      >
        {{ t('approval.approve') }}
      </button>
      <button
        class="btn btn-request-changes"
        :disabled="processing"
        @click="decide('request-changes')"
      >
        {{ t('approval.request_changes') }}
      </button>
      <button
        class="btn btn-deny"
        :disabled="processing"
        @click="decide('deny')"
      >
        {{ t('approval.deny') }}
      </button>
    </div>
  </div>
</template>

<style scoped>
.approval-actions {
  border: 2px solid var(--color-warning);
  border-radius: 8px;
  padding: 1rem;
  margin-top: 1.5rem;
  background: #fffbeb;
}
.approval-actions h4 {
  margin-bottom: 0.75rem;
  color: #92400e;
}
.comment-field {
  margin-bottom: 0.75rem;
}
.comment-input {
  width: 100%;
  padding: 0.5rem;
  border: 1px solid var(--border-color);
  border-radius: 6px;
  font-family: inherit;
  font-size: 0.875rem;
  resize: vertical;
}
.error-msg {
  color: var(--color-error);
  font-size: 0.8125rem;
  margin-bottom: 0.5rem;
}
.action-buttons {
  display: flex;
  gap: 0.5rem;
}
.btn-approve {
  background: var(--color-success);
  color: white;
}
.btn-deny {
  background: var(--color-error);
  color: white;
}
.btn-request-changes {
  background: var(--color-warning);
  color: white;
}
</style>
