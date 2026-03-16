<script setup lang="ts">
import Sidebar from './Sidebar.vue'
import Topbar from './Topbar.vue'
import ToastContainer from './ToastContainer.vue'
</script>

<template>
  <div class="app-layout">
    <Sidebar />
    <div class="main-content">
      <Topbar />
      <main class="page-content">
        <router-view />
      </main>
    </div>
    <ToastContainer />
  </div>
</template>

<style scoped>
.app-layout {
  display: flex;
  height: 100vh;
}
.main-content {
  flex: 1;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}
.page-content {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding: 1.5rem;
  display: flex;
  flex-direction: column;
}
</style>
