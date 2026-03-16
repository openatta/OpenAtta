<script setup lang="ts">
import { ref, computed, onMounted, watch } from 'vue'
import { useUsageStore } from '../stores/usage'
import { useI18n } from 'vue-i18n'

const { t } = useI18n()
const store = useUsageStore()
const period = ref('30d')
const chartMode = ref<'tokens' | 'cost'>('tokens')

onMounted(() => {
  store.fetchSummary(period.value)
  store.fetchByModel(period.value)
  fetchDailyForPeriod()
})

watch(period, () => {
  store.fetchSummary(period.value)
  store.fetchByModel(period.value)
  fetchDailyForPeriod()
})

function fetchDailyForPeriod() {
  const end = new Date()
  const start = new Date()
  const days = period.value === '7d' ? 7 : period.value === '30d' ? 30 : 90
  start.setDate(start.getDate() - days)
  store.fetchDaily(start.toISOString().slice(0, 10), end.toISOString().slice(0, 10))
}

function formatCost(v: number): string {
  return '$' + v.toFixed(2)
}

function formatTokens(v: number): string {
  if (v >= 1_000_000) return (v / 1_000_000).toFixed(1) + 'M'
  if (v >= 1_000) return (v / 1_000).toFixed(1) + 'K'
  return v.toString()
}

// SVG bar chart
const chartBars = computed(() => {
  const data = store.daily
  if (!data.length) return []
  const values = data.map(d => chartMode.value === 'tokens' ? d.tokens : d.cost_usd)
  const max = Math.max(...values, 1)
  const barW = Math.max(8, Math.min(40, 600 / data.length - 4))
  return data.map((d, i) => ({
    x: i * (barW + 4) + 40,
    height: (values[i] / max) * 180,
    value: values[i],
    label: d.date.slice(5), // MM-DD
    day: d,
  }))
})

const chartWidth = computed(() => {
  const n = store.daily.length
  const barW = Math.max(8, Math.min(40, 600 / n - 4))
  return Math.max(600, n * (barW + 4) + 80)
})

const modelRows = computed(() => {
  const models = store.byModel.length ? store.byModel : (store.summary?.by_model || [])
  if (!models.length) return []
  const total = models.reduce((s, m) => s + m.cost_usd, 0) || 1
  return [...models]
    .sort((a, b) => b.cost_usd - a.cost_usd)
    .map(m => ({ ...m, share: ((m.cost_usd / total) * 100).toFixed(1) }))
})

function exportCsv() {
  store.exportCsv(period.value)
}
</script>

<template>
  <div class="usage-view">
    <div class="header-row">
      <h2>{{ t('usage.title') }}</h2>
      <div class="action-buttons">
        <select v-model="period" class="period-select">
          <option value="7d">{{ t('usage.last_7d') }}</option>
          <option value="30d">{{ t('usage.last_30d') }}</option>
          <option value="90d">{{ t('usage.last_90d') }}</option>
        </select>
        <button class="btn btn-secondary" @click="exportCsv">{{ t('common.export_csv') }}</button>
      </div>
    </div>

    <!-- Summary Cards -->
    <div class="stat-cards" v-if="store.summary">
      <div class="stat-card">
        <div class="label">{{ t('usage.total_cost') }}</div>
        <div class="value">{{ formatCost(store.summary.total_cost_usd) }}</div>
      </div>
      <div class="stat-card">
        <div class="label">{{ t('usage.total_tokens') }}</div>
        <div class="value">{{ formatTokens(store.summary.total_tokens) }}</div>
      </div>
      <div class="stat-card">
        <div class="label">{{ t('usage.input_tokens') }}</div>
        <div class="value">{{ formatTokens(store.summary.input_tokens) }}</div>
      </div>
      <div class="stat-card">
        <div class="label">{{ t('usage.output_tokens') }}</div>
        <div class="value">{{ formatTokens(store.summary.output_tokens) }}</div>
      </div>
      <div class="stat-card">
        <div class="label">{{ t('usage.requests') }}</div>
        <div class="value">{{ store.summary.request_count }}</div>
      </div>
    </div>

    <!-- Chart -->
    <div class="card" v-if="store.daily.length">
      <div class="chart-header">
        <h3>{{ t('usage.daily_trend') }}</h3>
        <div class="tab-bar" style="border-bottom:none;margin-bottom:0">
          <button :class="['tab-btn', chartMode === 'tokens' ? 'active' : '']" @click="chartMode = 'tokens'">{{ t('usage.tokens') }}</button>
          <button :class="['tab-btn', chartMode === 'cost' ? 'active' : '']" @click="chartMode = 'cost'">{{ t('usage.cost') }}</button>
        </div>
      </div>
      <div class="chart-scroll">
        <svg :width="chartWidth" height="240" class="bar-chart">
          <g v-for="(bar, i) in chartBars" :key="i">
            <rect
              :x="bar.x"
              :y="220 - bar.height"
              :width="Math.max(8, Math.min(40, 600 / store.daily.length - 4))"
              :height="Math.max(1, bar.height)"
              fill="var(--color-primary)"
              rx="3"
              opacity="0.8"
            >
              <title>{{ bar.day.date }}: {{ chartMode === 'cost' ? formatCost(bar.value) : formatTokens(bar.value) }}</title>
            </rect>
            <text
              :x="bar.x + Math.max(8, Math.min(40, 600 / store.daily.length - 4)) / 2"
              y="235"
              font-size="9"
              fill="var(--text-secondary)"
              text-anchor="middle"
            >{{ i % Math.ceil(store.daily.length / 15) === 0 ? bar.label : '' }}</text>
          </g>
        </svg>
      </div>
    </div>

    <!-- Model Breakdown -->
    <div class="card" v-if="modelRows.length" style="margin-top: 1rem">
      <h3 style="margin-bottom: 0.75rem">{{ t('usage.model_breakdown') }}</h3>
      <table>
        <thead>
          <tr>
            <th>{{ t('usage.model') }}</th>
            <th>{{ t('usage.cost') }}</th>
            <th>{{ t('usage.tokens') }}</th>
            <th>{{ t('usage.requests') }}</th>
            <th>{{ t('usage.share') }}</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="m in modelRows" :key="m.model">
            <td class="mono">{{ m.model }}</td>
            <td>{{ formatCost(m.cost_usd) }}</td>
            <td>{{ formatTokens(m.tokens) }}</td>
            <td>{{ m.request_count }}</td>
            <td>
              <div class="share-bar">
                <div class="share-fill" :style="{ width: m.share + '%' }"></div>
                <span>{{ m.share }}%</span>
              </div>
            </td>
          </tr>
        </tbody>
      </table>
    </div>

    <div v-if="!store.summary && !store.loading" class="card">
      <p class="empty">{{ t('usage.no_data') }}</p>
    </div>
  </div>
</template>

<style scoped>
.usage-view h2 { margin-bottom: 0; }
.period-select {
  padding: 0.375rem 0.75rem;
  border: 1px solid var(--border-color);
  border-radius: 6px;
  background: var(--bg-surface);
  color: var(--text-primary);
  font-size: 0.8125rem;
}
.chart-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 0.75rem;
}
.chart-header h3 { font-size: 0.9375rem; margin: 0; }
.chart-scroll { overflow-x: auto; }
.bar-chart { display: block; }
.share-bar {
  display: flex;
  align-items: center;
  gap: 0.5rem;
}
.share-fill {
  height: 6px;
  background: var(--color-primary);
  border-radius: 3px;
  min-width: 4px;
  max-width: 120px;
}
.share-bar span {
  font-size: 0.75rem;
  color: var(--text-secondary);
  white-space: nowrap;
}
</style>
