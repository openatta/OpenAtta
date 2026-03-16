<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import type { FlowDef } from '../../types/api'

const { t } = useI18n()

const props = defineProps<{
  flow: FlowDef
}>()

interface NodePos {
  x: number
  y: number
  w: number
  h: number
}

const nodeW = 160
const nodeH = 52
const gapX = 80
const gapY = 28
const padX = 40
const padY = 40

const nodes = computed(() => {
  const pos: Record<string, NodePos> = {}
  const visited = new Set<string>()
  const queue: { id: string; col: number }[] = [
    { id: props.flow.initial_state, col: 0 },
  ]
  const colRows: Record<number, number> = {}

  while (queue.length > 0) {
    const { id, col } = queue.shift()!
    if (visited.has(id)) continue
    visited.add(id)

    const row = colRows[col] || 0
    colRows[col] = row + 1

    pos[id] = {
      x: padX + col * (nodeW + gapX),
      y: padY + row * (nodeH + gapY),
      w: nodeW,
      h: nodeH,
    }

    const state = props.flow.states[id]
    if (state?.transitions) {
      for (const tr of state.transitions) {
        if (!visited.has(tr.to)) {
          queue.push({ id: tr.to, col: col + 1 })
        }
      }
    }
  }

  // Place unvisited nodes
  for (const id of Object.keys(props.flow.states)) {
    if (!pos[id]) {
      const col = Object.keys(colRows).length
      const row = colRows[col] || 0
      colRows[col] = row + 1
      pos[id] = {
        x: padX + col * (nodeW + gapX),
        y: padY + row * (nodeH + gapY),
        w: nodeW,
        h: nodeH,
      }
    }
  }
  return pos
})

const edges = computed(() => {
  const result: { from: string; to: string; label?: string }[] = []
  for (const [id, state] of Object.entries(props.flow.states)) {
    if (state.transitions) {
      for (const tr of state.transitions) {
        result.push({ from: id, to: tr.to, label: tr.when })
      }
    }
  }
  return result
})

const svgW = computed(() => {
  const maxX = Math.max(...Object.values(nodes.value).map(n => n.x + n.w))
  return maxX + padX * 2
})

const svgH = computed(() => {
  const maxY = Math.max(...Object.values(nodes.value).map(n => n.y + n.h))
  return maxY + padY * 2
})

function edgePath(from: NodePos, to: NodePos): string {
  const x1 = from.x + from.w
  const y1 = from.y + from.h / 2
  const x2 = to.x
  const y2 = to.y + to.h / 2
  const cx = (x1 + x2) / 2
  return `M${x1},${y1} C${cx},${y1} ${cx},${y2} ${x2},${y2}`
}

const typeTheme: Record<string, { bg: string; border: string; badge: string }> = {
  start:    { bg: '#dbeafe', border: '#3b82f6', badge: '#3b82f6' },
  end:      { bg: '#dcfce7', border: '#22c55e', badge: '#22c55e' },
  agent:    { bg: '#f3f4f6', border: '#6b7280', badge: '#6b7280' },
  gate:     { bg: '#fef3c7', border: '#f59e0b', badge: '#f59e0b' },
  parallel: { bg: '#ede9fe', border: '#8b5cf6', badge: '#8b5cf6' },
}

function theme(type?: string) {
  return typeTheme[type || ''] || typeTheme.agent
}

function subtitle(id: string): string {
  const s = props.flow.states[id]
  if (!s) return ''
  if (s.skill) return t('flow_graph.skill', { name: s.skill })
  if (s.agent) return t('flow_graph.agent', { name: s.agent })
  if ((s as any).gate?.approver_role) return t('flow_graph.approver', { role: (s as any).gate.approver_role })
  return ''
}

function isTerminal(type?: string) {
  return type === 'start' || type === 'end'
}
</script>

<template>
  <div class="flow-graph">
    <svg :width="svgW" :height="svgH" xmlns="http://www.w3.org/2000/svg">
      <defs>
        <marker id="arrow" markerWidth="8" markerHeight="6" refX="8" refY="3" orient="auto">
          <polygon points="0 0, 8 3, 0 6" fill="#9ca3af" />
        </marker>
      </defs>

      <!-- Edges -->
      <g v-for="(edge, i) in edges" :key="`e-${i}`">
        <template v-if="nodes[edge.from] && nodes[edge.to]">
          <path
            :d="edgePath(nodes[edge.from], nodes[edge.to])"
            fill="none"
            stroke="#c4c9d4"
            stroke-width="1.5"
            marker-end="url(#arrow)"
          />
          <text
            v-if="edge.label"
            :x="(nodes[edge.from].x + nodes[edge.from].w + nodes[edge.to].x) / 2"
            :y="(nodes[edge.from].y + nodes[edge.to].y) / 2 + nodeH / 2 - 8"
            font-size="10"
            fill="#9ca3af"
            text-anchor="middle"
            font-style="italic"
          >{{ edge.label }}</text>
        </template>
      </g>

      <!-- Nodes -->
      <g v-for="id in Object.keys(nodes)" :key="id">
        <rect
          :x="nodes[id].x"
          :y="nodes[id].y"
          :width="nodes[id].w"
          :height="nodes[id].h"
          :fill="theme(flow.states[id]?.type).bg"
          :stroke="theme(flow.states[id]?.type).border"
          stroke-width="1.5"
          :rx="isTerminal(flow.states[id]?.type) ? 24 : 8"
        />
        <!-- Type badge -->
        <rect
          :x="nodes[id].x + 6"
          :y="nodes[id].y + 6"
          :width="flow.states[id]?.type?.length * 6.5 + 8 || 30"
          height="14"
          :fill="theme(flow.states[id]?.type).badge"
          rx="3"
          opacity="0.85"
        />
        <text
          :x="nodes[id].x + 10"
          :y="nodes[id].y + 15.5"
          font-size="9"
          font-weight="600"
          fill="#fff"
        >{{ flow.states[id]?.type }}</text>
        <!-- Name -->
        <text
          :x="nodes[id].x + nodes[id].w / 2"
          :y="nodes[id].y + nodes[id].h / 2 + 2"
          font-size="12.5"
          font-weight="600"
          fill="#1f2937"
          text-anchor="middle"
        >{{ id }}</text>
        <!-- Subtitle -->
        <text
          v-if="subtitle(id)"
          :x="nodes[id].x + nodes[id].w / 2"
          :y="nodes[id].y + nodes[id].h / 2 + 16"
          font-size="10"
          fill="#6b7280"
          text-anchor="middle"
        >{{ subtitle(id) }}</text>
      </g>
    </svg>
  </div>
</template>

<style scoped>
.flow-graph {
  overflow: auto;
  background: #fafafa;
  border: 1px solid var(--border-color);
  border-radius: 8px;
  padding: 0.5rem;
}
</style>
