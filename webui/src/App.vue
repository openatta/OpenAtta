<script setup lang="ts">
import { onMounted } from 'vue'
import AppLayout from './components/layout/AppLayout.vue'
import { useThemeStore } from './stores/theme'

const theme = useThemeStore()
onMounted(() => theme.apply())
</script>

<template>
  <AppLayout />
</template>
