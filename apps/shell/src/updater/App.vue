<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

// ── Tab state ──
type Tab = "shell" | "component";
const activeTab = ref<Tab>("shell");

// ── Shell Update state ──
type ShellState = "idle" | "checking" | "available" | "downloading" | "done" | "error";
const shellState = ref<ShellState>("idle");
const shellError = ref("");
const updateInfo = ref<{ current_version: string; version: string; body: string } | null>(null);
const shellProgress = ref(0);

async function checkShellUpdate() {
  shellState.value = "checking";
  shellError.value = "";
  try {
    const info = await invoke<{ current_version: string; version: string; body: string } | null>(
      "check_update"
    );
    if (info) {
      updateInfo.value = info;
      shellState.value = "available";
    } else {
      shellState.value = "done";
    }
  } catch (e: unknown) {
    shellError.value = String(e);
    shellState.value = "error";
  }
}

async function installShellUpdate() {
  shellState.value = "downloading";
  shellProgress.value = 0;
  try {
    await invoke("install_update");
    shellState.value = "done";
  } catch (e: unknown) {
    shellError.value = String(e);
    shellState.value = "error";
  }
}

// ── Download speed / ETA tracking ──
const downloadSpeed = ref("");
const downloadEta = ref("");
let lastDownloadBytes = 0;
let lastDownloadTime = 0;

function updateDownloadStats(downloaded: number, total: number | null) {
  const now = Date.now();
  if (lastDownloadTime > 0) {
    const deltaTime = (now - lastDownloadTime) / 1000; // seconds
    const deltaBytes = downloaded - lastDownloadBytes;
    if (deltaTime > 0.3) {
      const speed = deltaBytes / deltaTime;
      downloadSpeed.value = formatSpeed(speed);
      if (total && total > 0) {
        const remaining = total - downloaded;
        const etaSecs = remaining / speed;
        downloadEta.value = etaSecs > 0 ? `~${Math.ceil(etaSecs)}s remaining` : "";
      }
      lastDownloadBytes = downloaded;
      lastDownloadTime = now;
    }
  } else {
    lastDownloadBytes = downloaded;
    lastDownloadTime = now;
  }
}

function formatSpeed(bytesPerSec: number): string {
  if (bytesPerSec >= 1048576) return (bytesPerSec / 1048576).toFixed(1) + " MB/s";
  if (bytesPerSec >= 1024) return (bytesPerSec / 1024).toFixed(0) + " KB/s";
  return bytesPerSec.toFixed(0) + " B/s";
}

function resetDownloadStats() {
  downloadSpeed.value = "";
  downloadEta.value = "";
  lastDownloadBytes = 0;
  lastDownloadTime = 0;
}

// ── Error classification ──
function classifyError(err: string): { icon: string; hint: string; color: string } {
  if (err.startsWith("[NETWORK]"))
    return { icon: "!!", hint: "Check your internet connection and try again.", color: "#ffa500" };
  if (err.startsWith("[DISK]"))
    return { icon: "!!", hint: "Free up disk space and try again.", color: "#ff6b6b" };
  if (err.startsWith("[PERMISSION]"))
    return { icon: "!!", hint: "Check directory permissions.", color: "#ff6b6b" };
  if (err.startsWith("[INTEGRITY]"))
    return { icon: "!!", hint: "Package integrity check failed. Try downloading again.", color: "#ff6b6b" };
  return { icon: "!!", hint: "", color: "#ff6b6b" };
}

const compErrorInfo = computed(() => classifyError(compError.value));

// ── Component Update state ──
type CompState = "idle" | "checking" | "available" | "upgrading" | "done" | "error";
const compState = ref<CompState>("idle");
const compError = ref("");
const compInfo = ref<{
  available: boolean;
  current_version: string;
  remote_version: string;
  package_url: string | null;
  sha256: string | null;
  min_shell_version: string | null;
  shell_compatible: boolean;
} | null>(null);
const compPhase = ref("");
const compDetail = ref("");
const manifestUrl = ref("https://releases.attaos.dev/latest/manifest.json");

async function checkComponentUpdate() {
  compState.value = "checking";
  compError.value = "";
  try {
    const home = await invoke<string>("get_default_home");
    const info = await invoke<{
      available: boolean;
      current_version: string;
      remote_version: string;
      package_url: string | null;
      sha256: string | null;
      min_shell_version: string | null;
      shell_compatible: boolean;
    }>("check_component_update", { home, manifestUrl: manifestUrl.value });

    compInfo.value = info;
    if (info.available && !info.shell_compatible) {
      compError.value = `Update requires shell version ${info.min_shell_version}. Please update the shell first (Shell Update tab).`;
      compState.value = "error";
    } else {
      compState.value = info.available ? "available" : "done";
    }
  } catch (e: unknown) {
    compError.value = String(e);
    compState.value = "error";
  }
}

async function startComponentUpgrade() {
  if (!compInfo.value?.package_url) {
    compError.value = "No package URL in manifest";
    compState.value = "error";
    return;
  }

  compState.value = "upgrading";
  compPhase.value = "starting";
  compDetail.value = "";
  resetDownloadStats();

  try {
    const home = await invoke<string>("get_default_home");

    // Disk space precheck
    const space = await invoke<{ available: number; total: number; sufficient: boolean }>(
      "check_disk_space",
      { path: home }
    );
    if (!space.sufficient) {
      const avail = space.available >= 1048576
        ? (space.available / 1048576).toFixed(0) + " MB"
        : (space.available / 1024).toFixed(0) + " KB";
      throw new Error(`Insufficient disk space: ${avail} available, need at least 500 MB`);
    }

    // Fetch the full remote manifest for write_manifest
    const manifest = await invoke<Record<string, unknown>>("fetch_manifest", {
      url: manifestUrl.value,
    });

    await invoke("upgrade_components", {
      home,
      packageUrl: compInfo.value.package_url,
      sha256: compInfo.value.sha256,
      manifest,
    });

    compState.value = "done";
  } catch (e: unknown) {
    compError.value = String(e);
    compState.value = "error";
  }
}

async function cancelDownload() {
  try {
    await invoke("cancel_download");
  } catch (e) {
    console.error("cancel failed:", e);
  }
}

// ── Event listeners ──
const unlisteners: Array<() => void> = [];

onMounted(async () => {
  const unUpgrade = await listen<{ phase: string; detail: string }>(
    "upgrade-progress",
    (event) => {
      compPhase.value = event.payload.phase;
      compDetail.value = event.payload.detail;
    }
  );
  unlisteners.push(unUpgrade);

  const unDownload = await listen<{ downloaded: number; total?: number; percent?: number }>(
    "download-progress",
    (event) => {
      updateDownloadStats(event.payload.downloaded, event.payload.total ?? null);
    }
  );
  unlisteners.push(unDownload);

  // Shell update progress
  const unUpdateProgress = await listen<{ downloaded: number; total?: number; percent?: number }>(
    "update-progress",
    (event) => {
      shellProgress.value = event.payload.percent ?? 0;
    }
  );
  unlisteners.push(unUpdateProgress);
});

onUnmounted(() => {
  unlisteners.forEach((fn) => fn());
});
</script>

<template>
  <div class="updater">
    <h1>AttaOS Updater</h1>

    <!-- Tab bar -->
    <div class="tab-bar">
      <button
        :class="['tab', { active: activeTab === 'shell' }]"
        @click="activeTab = 'shell'"
      >
        Shell Update
      </button>
      <button
        :class="['tab', { active: activeTab === 'component' }]"
        @click="activeTab = 'component'"
      >
        Component Update
      </button>
    </div>

    <!-- Shell Update Tab -->
    <div v-if="activeTab === 'shell'">
      <div v-if="shellState === 'idle'" class="section">
        <p>Check for shell (attash) updates.</p>
        <button @click="checkShellUpdate">Check for Updates</button>
      </div>

      <div v-else-if="shellState === 'checking'" class="section">
        <div class="spinner"></div>
        <p>Checking for updates...</p>
      </div>

      <div v-else-if="shellState === 'available' && updateInfo" class="section">
        <p>
          Update available: <strong>{{ updateInfo.version }}</strong>
          (current: {{ updateInfo.current_version }})
        </p>
        <p v-if="updateInfo.body" class="release-notes">{{ updateInfo.body }}</p>
        <button @click="installShellUpdate">Install Update</button>
      </div>

      <div v-else-if="shellState === 'downloading'" class="section">
        <p>Downloading and installing...</p>
        <div class="progress-bar">
          <div class="progress-fill" :style="{ width: shellProgress + '%' }"></div>
        </div>
      </div>

      <div v-else-if="shellState === 'done'" class="section">
        <p v-if="updateInfo">Update installed successfully. Please restart AttaOS.</p>
        <p v-else>You are already on the latest version.</p>
      </div>

      <div v-else-if="shellState === 'error'" class="section error">
        <p>Error: {{ shellError }}</p>
        <button @click="shellState = 'idle'">Retry</button>
      </div>
    </div>

    <!-- Component Update Tab -->
    <div v-if="activeTab === 'component'">
      <div v-if="compState === 'idle'" class="section">
        <p>Check for server component updates (attaos, WebUI, skills, flows).</p>
        <button @click="checkComponentUpdate">Check for Updates</button>
      </div>

      <div v-else-if="compState === 'checking'" class="section">
        <div class="spinner"></div>
        <p>Checking component versions...</p>
      </div>

      <div v-else-if="compState === 'available' && compInfo" class="section">
        <p>
          Component update available: <strong>{{ compInfo.remote_version }}</strong>
          (current: {{ compInfo.current_version }})
        </p>
        <button @click="startComponentUpgrade">Upgrade</button>
      </div>

      <div v-else-if="compState === 'upgrading'" class="section">
        <div class="spinner"></div>
        <p>{{ compDetail || 'Upgrading...' }}</p>
        <p class="info">Phase: {{ compPhase }}</p>
        <p v-if="downloadSpeed" class="info">{{ downloadSpeed }} <span v-if="downloadEta">— {{ downloadEta }}</span></p>
        <button v-if="compPhase === 'download'" class="secondary" @click="cancelDownload">Cancel</button>
      </div>

      <div v-else-if="compState === 'done'" class="section">
        <p v-if="compInfo?.available">Components upgraded successfully!</p>
        <p v-else>Components are up to date.</p>
        <button @click="compState = 'idle'">Check Again</button>
      </div>

      <div v-else-if="compState === 'error'" class="section error">
        <p :style="{ color: compErrorInfo.color }">Error: {{ compError }}</p>
        <p v-if="compErrorInfo.hint" class="info">{{ compErrorInfo.hint }}</p>
        <button @click="compState = 'idle'">Retry</button>
      </div>
    </div>
  </div>
</template>

<style>
* {
  margin: 0;
  padding: 0;
  box-sizing: border-box;
}

body {
  font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
  background: #1a1a2e;
  color: #e0e0e0;
}

.updater {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  min-height: 100vh;
  padding: 24px;
  text-align: center;
}

h1 {
  font-size: 20px;
  margin-bottom: 16px;
  color: #fff;
}

/* Tab bar */
.tab-bar {
  display: flex;
  gap: 0;
  margin-bottom: 24px;
  border-radius: 6px;
  overflow: hidden;
  border: 1px solid #333;
}

.tab {
  background: #16213e;
  color: #999;
  border: none;
  border-radius: 0;
  padding: 8px 20px;
  font-size: 13px;
  cursor: pointer;
  transition: background 0.2s, color 0.2s;
}

.tab:hover {
  background: #1a2744;
  color: #e0e0e0;
}

.tab.active {
  background: #6c63ff;
  color: #fff;
}

.section {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 12px;
}

.info {
  font-size: 13px;
  color: #888;
}

button {
  background: #6c63ff;
  color: #fff;
  border: none;
  border-radius: 6px;
  padding: 10px 24px;
  font-size: 14px;
  cursor: pointer;
  transition: background 0.2s;
}

button:hover {
  background: #5a52d5;
}

.spinner {
  width: 32px;
  height: 32px;
  border: 3px solid #333;
  border-top-color: #6c63ff;
  border-radius: 50%;
  animation: spin 0.8s linear infinite;
}

@keyframes spin {
  to {
    transform: rotate(360deg);
  }
}

.progress-bar {
  width: 280px;
  height: 8px;
  background: #333;
  border-radius: 4px;
  overflow: hidden;
}

.progress-fill {
  height: 100%;
  background: #6c63ff;
  border-radius: 4px;
  transition: width 0.3s;
}

.release-notes {
  font-size: 13px;
  color: #999;
  max-width: 360px;
  white-space: pre-wrap;
}

.error p {
  color: #ff6b6b;
}
</style>
