import { createRouter, createWebHistory } from 'vue-router'

const router = createRouter({
  history: createWebHistory(),
  routes: [
    {
      path: '/',
      name: 'dashboard',
      component: () => import('../views/DashboardView.vue'),
    },
    {
      path: '/tasks',
      name: 'tasks',
      component: () => import('../views/TasksView.vue'),
    },
    {
      path: '/tasks/:id',
      name: 'task-detail',
      component: () => import('../views/TaskDetailView.vue'),
    },
    {
      path: '/flows',
      name: 'flows',
      component: () => import('../views/FlowsView.vue'),
    },
    {
      path: '/skills',
      name: 'skills',
      component: () => import('../views/SkillsView.vue'),
    },
    {
      path: '/tools',
      name: 'tools',
      component: () => import('../views/ToolsView.vue'),
    },
    {
      path: '/mcp',
      name: 'mcp',
      component: () => import('../views/McpView.vue'),
    },
    {
      path: '/chat',
      name: 'chat',
      component: () => import('../views/ChatView.vue'),
    },
    {
      path: '/agents',
      name: 'agents',
      component: () => import('../views/AgentsView.vue'),
    },
    {
      path: '/settings',
      name: 'settings',
      component: () => import('../views/SettingsView.vue'),
    },
    // New routes
    {
      path: '/cron',
      name: 'cron',
      component: () => import('../views/CronView.vue'),
    },
    {
      path: '/channels',
      name: 'channels',
      component: () => import('../views/ChannelsView.vue'),
    },
    {
      path: '/usage',
      name: 'usage',
      component: () => import('../views/UsageView.vue'),
    },
    {
      path: '/logs',
      name: 'logs',
      component: () => import('../views/LogsView.vue'),
    },
    {
      path: '/memory',
      name: 'memory',
      component: () => import('../views/MemoryView.vue'),
    },
    {
      path: '/audit',
      name: 'audit',
      component: () => import('../views/AuditView.vue'),
    },
    {
      path: '/approvals',
      name: 'approvals',
      component: () => import('../views/ApprovalsView.vue'),
    },
    {
      path: '/diagnostics',
      name: 'diagnostics',
      component: () => import('../views/DiagnosticsView.vue'),
    },
    {
      path: '/:pathMatch(.*)*',
      name: 'not-found',
      component: () => import('../views/NotFoundView.vue'),
    },
  ],
})

export default router
