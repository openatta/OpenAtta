<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import { useRouter } from 'vue-router'

const { t } = useI18n()
const router = useRouter()

function goHome() {
  router.push({ name: 'dashboard' })
}
</script>

<template>
  <div class="not-found">
    <div class="not-found-content">
      <h1 class="not-found-code">404</h1>
      <h2 class="not-found-title">{{ t('not_found.title') }}</h2>
      <p class="not-found-message">{{ t('not_found.message') }}</p>
      <button class="btn btn-primary" @click="goHome">
        {{ t('not_found.back_home') }}
      </button>
    </div>
  </div>
</template>

<style scoped>
.not-found {
  display: flex;
  align-items: center;
  justify-content: center;
  min-height: 60vh;
  text-align: center;
}

.not-found-content {
  max-width: 420px;
}

.not-found-code {
  font-size: 6rem;
  font-weight: 700;
  line-height: 1;
  margin: 0 0 0.5rem;
  color: var(--text-secondary);
  opacity: 0.4;
}

.not-found-title {
  font-size: 1.5rem;
  font-weight: 600;
  margin: 0 0 0.75rem;
  color: var(--text-primary);
}

.not-found-message {
  font-size: 0.875rem;
  color: var(--text-secondary);
  margin: 0 0 1.5rem;
}
</style>
