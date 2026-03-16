<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";

// ── State machine ──
// welcome → mode_select → [local_install | connect_server]
// local_install: install_dir → source_select → installing → complete
// connect_server: server_url → testing → complete

type Step =
  | "welcome"
  | "mode_select"
  // local install flow
  | "install_dir"
  | "source_select"
  | "config"
  | "installing"
  // connect server flow
  | "server_url"
  | "testing"
  // shared
  | "complete"
  | "error";

const step = ref<Step>("welcome");
const previousStep = ref<Step>("mode_select");
const errorMsg = ref("");
const unlisteners: Array<() => void> = [];

// ── Install data ──
const platform = ref("");
const defaultHome = ref("");
const installDir = ref("");
const diskSpace = ref({ available: 0, total: 0, sufficient: true });

// Source selection
type SourceType = "url" | "file";
const sourceType = ref<SourceType>("url");
const manifestUrl = ref("https://releases.attaos.dev/latest/manifest.json");
const localFilePath = ref("");

// Config
const llmProvider = ref("auto");
const apiKey = ref("");
const serverPort = ref(3000);

// Download/extract progress
const downloadProgress = ref(0);
const extractProgress = ref(0);
const installPhase = ref<"downloading" | "verifying" | "extracting" | "deploying" | "starting" | "done">("downloading");

// ── Connection data ──
const serverUrl = ref("http://localhost:3000");
const serverName = ref("default");
const authType = ref("none");
const connectionResult = ref<{ reachable: boolean; version?: string; error?: string } | null>(null);

// ── Helpers ──

function showError(msg: string) {
  previousStep.value = step.value as Step;
  errorMsg.value = msg;
  step.value = "error";
}

// ── Lifecycle ──

onMounted(async () => {
  try {
    platform.value = await invoke<string>("get_platform");
    defaultHome.value = await invoke<string>("get_default_home");
    installDir.value = defaultHome.value;
  } catch (e) {
    console.error("init failed:", e);
  }

  // Listen for progress events (save unlisten handles)
  const unDownload = await listen<{ downloaded: number; total?: number; percent?: number }>(
    "download-progress",
    (event) => {
      downloadProgress.value = event.payload.percent ?? 0;
      updateDownloadStats(event.payload.downloaded, event.payload.total ?? null);
    }
  );
  unlisteners.push(unDownload);

  const unExtract = await listen<{ extracted: number; current_file: string }>(
    "extract-progress",
    (event) => {
      extractProgress.value = event.payload.extracted;
    }
  );
  unlisteners.push(unExtract);
});

onUnmounted(() => {
  unlisteners.forEach((fn) => fn());
});

// ── Actions ──

function goToModeSelect() {
  step.value = "mode_select";
}

function selectLocalInstall() {
  step.value = "install_dir";
}

function selectConnectServer() {
  step.value = "server_url";
}

async function checkDiskAndProceed() {
  try {
    const space = await invoke<{ available: number; total: number; sufficient: boolean }>(
      "check_disk_space",
      { path: installDir.value }
    );
    diskSpace.value = space;
    if (!space.sufficient) {
      showError(`Insufficient disk space. Available: ${formatBytes(space.available)}, need at least 500 MB.`);
      return;
    }
    step.value = "source_select";
  } catch (e) {
    showError(String(e));
  }
}

async function selectLocalFile() {
  try {
    const path = await invoke<string | null>("select_file");
    if (path) {
      localFilePath.value = path;
    }
  } catch (e) {
    errorMsg.value = String(e);
  }
}

async function startInstall() {
  step.value = "installing";
  installPhase.value = "downloading";
  downloadProgress.value = 0;
  extractProgress.value = 0;
  resetDownloadStats();

  try {
    const tmpDir = `${installDir.value}/tmp`;

    if (sourceType.value === "url") {
      // Download manifest
      installPhase.value = "downloading";
      const manifest = await invoke<Record<string, unknown>>("fetch_manifest", {
        url: manifestUrl.value,
      });

      // Download package
      const packageUrl = (manifest as Record<string, unknown>)["package_url"] as string | undefined;
      if (!packageUrl) {
        throw new Error("manifest missing package_url");
      }
      const destFile = `${tmpDir}/package.tar.gz`;
      await invoke("download_components", { url: packageUrl, dest: destFile });

      // Verify
      installPhase.value = "verifying";
      const expectedSha = (manifest as Record<string, unknown>)["sha256"] as string | undefined;
      if (expectedSha) {
        const valid = await invoke<boolean>("verify_package", {
          path: destFile,
          sha256: expectedSha,
        });
        if (!valid) {
          throw new Error("SHA-256 verification failed");
        }
      }

      // Extract
      installPhase.value = "extracting";
      await invoke("extract_package", { tarGz: destFile, dest: `${tmpDir}/extracted` });

      // Deploy
      installPhase.value = "deploying";
      await invoke("install_components", {
        packageDir: `${tmpDir}/extracted`,
        home: installDir.value,
      });

      // Generate config files (before manifest so partial state is recoverable)
      await invoke("generate_config", {
        home: installDir.value,
        config: {
          provider: llmProvider.value,
          api_key: apiKey.value,
          port: serverPort.value,
        },
      });

      // Write manifest (marks installation as complete)
      await invoke("write_manifest", { home: installDir.value, manifest });

    } else {
      // Local file: extract directly
      installPhase.value = "extracting";
      await invoke("extract_package", {
        tarGz: localFilePath.value,
        dest: `${tmpDir}/extracted`,
      });

      // Deploy
      installPhase.value = "deploying";
      await invoke("install_components", {
        packageDir: `${tmpDir}/extracted`,
        home: installDir.value,
      });

      // Generate config files (before manifest so partial state is recoverable)
      await invoke("generate_config", {
        home: installDir.value,
        config: {
          provider: llmProvider.value,
          api_key: apiKey.value,
          port: serverPort.value,
        },
      });

      // Write a basic manifest (marks installation as complete)
      await invoke("write_manifest", {
        home: installDir.value,
        manifest: {
          version: "local",
          installed_at: new Date().toISOString(),
          source: "local_file",
        },
      });
    }

    // Start server
    installPhase.value = "starting";
    await invoke("start_server", { home: installDir.value, port: serverPort.value });

    // Cleanup tmp directory
    try {
      await invoke("cleanup_tmp", { home: installDir.value });
    } catch (_) {
      // Non-fatal: tmp cleanup failure shouldn't block completion
    }

    // Clear sensitive data from memory
    apiKey.value = "";

    installPhase.value = "done";
    step.value = "complete";
  } catch (e) {
    // Attempt tmp cleanup even on failure
    try {
      await invoke("cleanup_tmp", { home: installDir.value });
    } catch (_) { /* ignore */ }
    apiKey.value = "";
    showError(String(e));
  }
}

async function testAndConnect() {
  step.value = "testing";
  connectionResult.value = null;

  try {
    const result = await invoke<{ reachable: boolean; version?: string; error?: string }>(
      "test_connection",
      { url: serverUrl.value }
    );
    connectionResult.value = result;

    if (result.reachable) {
      // Save connection
      await invoke("save_connection", {
        home: defaultHome.value,
        name: serverName.value,
        url: serverUrl.value,
        authType: authType.value,
      });
      step.value = "complete";
    } else {
      showError(result.error || "Server unreachable");
    }
  } catch (e) {
    showError(String(e));
  }
}

async function openConsole() {
  // Navigate the main window to the server via Tauri API
  const url = connectionResult.value ? serverUrl.value : `http://localhost:${serverPort.value}`;
  const webview = getCurrentWebviewWindow();
  await webview.navigate(url);
}

function goBack() {
  if (step.value === "error") {
    step.value = previousStep.value;
    errorMsg.value = "";
  }
}

function formatBytes(bytes: number): string {
  if (bytes >= 1073741824) return (bytes / 1073741824).toFixed(1) + " GB";
  if (bytes >= 1048576) return (bytes / 1048576).toFixed(0) + " MB";
  return (bytes / 1024).toFixed(0) + " KB";
}

// ── Download speed / ETA tracking ──
const downloadSpeed = ref("");
const downloadEta = ref("");
let lastDlBytes = 0;
let lastDlTime = 0;

function updateDownloadStats(downloaded: number, total: number | null) {
  const now = Date.now();
  if (lastDlTime > 0) {
    const deltaTime = (now - lastDlTime) / 1000;
    const deltaBytes = downloaded - lastDlBytes;
    if (deltaTime > 0.3) {
      const speed = deltaBytes / deltaTime;
      downloadSpeed.value =
        speed >= 1048576 ? (speed / 1048576).toFixed(1) + " MB/s" :
        speed >= 1024 ? (speed / 1024).toFixed(0) + " KB/s" :
        speed.toFixed(0) + " B/s";
      if (total && total > 0) {
        const remaining = total - downloaded;
        const etaSecs = remaining / speed;
        downloadEta.value = etaSecs > 0 ? `~${Math.ceil(etaSecs)}s remaining` : "";
      }
      lastDlBytes = downloaded;
      lastDlTime = now;
    }
  } else {
    lastDlBytes = downloaded;
    lastDlTime = now;
  }
}

function resetDownloadStats() {
  downloadSpeed.value = "";
  downloadEta.value = "";
  lastDlBytes = 0;
  lastDlTime = 0;
}

// ── Error classification ──
function classifyError(msg: string): { hint: string; color: string } {
  if (msg.startsWith("[NETWORK]"))
    return { hint: "Check your internet connection and try again.", color: "#ffa500" };
  if (msg.startsWith("[DISK]"))
    return { hint: "Free up disk space and try again.", color: "#ff6b6b" };
  if (msg.startsWith("[PERMISSION]"))
    return { hint: "Check directory permissions.", color: "#ff6b6b" };
  if (msg.startsWith("[INTEGRITY]"))
    return { hint: "Package integrity check failed. Try again.", color: "#ff6b6b" };
  return { hint: "", color: "#ff6b6b" };
}

const errorInfo = computed(() => classifyError(errorMsg.value));

async function cancelDownload() {
  try { await invoke("cancel_download"); } catch (e) { console.error(e); }
}
</script>

<template>
  <div class="installer">
    <h1>AttaOS</h1>

    <!-- Welcome -->
    <div v-if="step === 'welcome'" class="section">
      <p class="subtitle">AI Operating System</p>
      <p>Welcome to AttaOS. Let's get you set up.</p>
      <p class="info">Platform: {{ platform }}</p>
      <button @click="goToModeSelect">Get Started</button>
    </div>

    <!-- Mode Select -->
    <div v-else-if="step === 'mode_select'" class="section">
      <p>How would you like to use AttaOS?</p>
      <div class="mode-cards">
        <div class="mode-card" @click="selectLocalInstall">
          <h3>Local Install</h3>
          <p>Install AttaOS on this machine. Includes server, WebUI, and all components.</p>
        </div>
        <div class="mode-card" @click="selectConnectServer">
          <h3>Connect to Server</h3>
          <p>Connect to an existing AttaOS server on your network.</p>
        </div>
      </div>
    </div>

    <!-- Install Dir -->
    <div v-else-if="step === 'install_dir'" class="section">
      <p>Installation Directory</p>
      <div class="input-group">
        <input v-model="installDir" type="text" placeholder="Installation path" />
      </div>
      <p class="info">Default: {{ defaultHome }}</p>
      <div class="button-row">
        <button class="secondary" @click="step = 'mode_select'">Back</button>
        <button @click="checkDiskAndProceed">Next</button>
      </div>
    </div>

    <!-- Source Select -->
    <div v-else-if="step === 'source_select'" class="section">
      <p>Installation Source</p>
      <div class="radio-group">
        <label>
          <input type="radio" v-model="sourceType" value="url" />
          Download from URL
        </label>
        <label>
          <input type="radio" v-model="sourceType" value="file" />
          Local file (.tar.gz)
        </label>
      </div>

      <div v-if="sourceType === 'url'" class="input-group">
        <input v-model="manifestUrl" type="text" placeholder="Manifest URL" />
      </div>
      <div v-else class="input-group">
        <input v-model="localFilePath" type="text" placeholder="Path to .tar.gz" readonly />
        <button class="small" @click="selectLocalFile">Browse</button>
      </div>

      <div class="button-row">
        <button class="secondary" @click="step = 'install_dir'">Back</button>
        <button @click="step = 'config'" :disabled="sourceType === 'file' && !localFilePath">
          Next
        </button>
      </div>
    </div>

    <!-- Config -->
    <div v-else-if="step === 'config'" class="section">
      <p>Configuration (optional)</p>
      <div class="input-group">
        <label>LLM Provider</label>
        <select v-model="llmProvider">
          <option value="auto">Auto Detect</option>
          <option value="anthropic">Anthropic (Claude)</option>
          <option value="openai">OpenAI</option>
          <option value="deepseek">DeepSeek</option>
        </select>
      </div>
      <div class="input-group">
        <label>API Key</label>
        <input v-model="apiKey" type="password" placeholder="sk-..." />
      </div>
      <div class="input-group">
        <label>Server Port</label>
        <input v-model.number="serverPort" type="number" min="1024" max="65535" />
      </div>
      <p class="info">You can change these settings later in $ATTA_HOME/etc/</p>
      <div class="button-row">
        <button class="secondary" @click="step = 'source_select'">Back</button>
        <button class="secondary" @click="startInstall">Skip</button>
        <button @click="startInstall">Install</button>
      </div>
    </div>

    <!-- Installing -->
    <div v-else-if="step === 'installing'" class="section">
      <div class="spinner"></div>
      <p v-if="installPhase === 'downloading'">Downloading components...</p>
      <p v-else-if="installPhase === 'verifying'">Verifying package integrity...</p>
      <p v-else-if="installPhase === 'extracting'">Extracting files...</p>
      <p v-else-if="installPhase === 'deploying'">Deploying to {{ installDir }}...</p>
      <p v-else-if="installPhase === 'starting'">Starting AttaOS server...</p>

      <div v-if="installPhase === 'downloading'" class="progress-bar">
        <div class="progress-fill" :style="{ width: downloadProgress + '%' }"></div>
      </div>
      <p v-if="installPhase === 'downloading' && downloadSpeed" class="info">
        {{ downloadSpeed }} <span v-if="downloadEta">— {{ downloadEta }}</span>
      </p>
      <button v-if="installPhase === 'downloading'" class="secondary" @click="cancelDownload">Cancel</button>
      <p v-if="installPhase === 'extracting'" class="info">
        {{ extractProgress }} files extracted
      </p>
    </div>

    <!-- Server URL -->
    <div v-else-if="step === 'server_url'" class="section">
      <p>Remote Server</p>
      <div class="input-group">
        <label>Connection Name</label>
        <input v-model="serverName" type="text" placeholder="e.g. production" />
      </div>
      <div class="input-group">
        <label>Server URL</label>
        <input v-model="serverUrl" type="text" placeholder="http://hostname:3000" />
      </div>
      <div class="input-group">
        <label>Authentication</label>
        <select v-model="authType">
          <option value="none">None</option>
          <option value="token">API Token</option>
        </select>
      </div>
      <div class="button-row">
        <button class="secondary" @click="step = 'mode_select'">Back</button>
        <button @click="testAndConnect">Connect</button>
      </div>
    </div>

    <!-- Testing connection -->
    <div v-else-if="step === 'testing'" class="section">
      <div class="spinner"></div>
      <p>Testing connection to {{ serverUrl }}...</p>
    </div>

    <!-- Complete -->
    <div v-else-if="step === 'complete'" class="section">
      <div class="checkmark">&#10003;</div>
      <p>Setup complete!</p>
      <p v-if="connectionResult?.version" class="info">
        Connected to AttaOS {{ connectionResult.version }}
      </p>
      <button @click="openConsole">Open Console</button>
    </div>

    <!-- Error -->
    <div v-else-if="step === 'error'" class="section error">
      <p :style="{ color: errorInfo.color }">{{ errorMsg }}</p>
      <p v-if="errorInfo.hint" class="info">{{ errorInfo.hint }}</p>
      <button @click="goBack">Back</button>
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

.installer {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  min-height: 100vh;
  padding: 32px;
  text-align: center;
}

h1 {
  font-size: 28px;
  margin-bottom: 8px;
  color: #fff;
}

.subtitle {
  font-size: 14px;
  color: #888;
  margin-bottom: 24px;
}

.section {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 16px;
  max-width: 480px;
  width: 100%;
}

.info {
  font-size: 13px;
  color: #888;
}

/* Mode cards */
.mode-cards {
  display: flex;
  gap: 16px;
  margin-top: 8px;
}

.mode-card {
  background: #16213e;
  border: 1px solid #333;
  border-radius: 8px;
  padding: 20px;
  cursor: pointer;
  transition: border-color 0.2s, background 0.2s;
  text-align: left;
  flex: 1;
}

.mode-card:hover {
  border-color: #6c63ff;
  background: #1a2744;
}

.mode-card h3 {
  font-size: 15px;
  color: #fff;
  margin-bottom: 8px;
}

.mode-card p {
  font-size: 13px;
  color: #999;
}

/* Inputs */
.input-group {
  width: 100%;
  display: flex;
  flex-direction: column;
  gap: 4px;
  text-align: left;
}

.input-group label {
  font-size: 13px;
  color: #999;
}

.input-group input,
.input-group select {
  background: #16213e;
  color: #e0e0e0;
  border: 1px solid #333;
  border-radius: 6px;
  padding: 10px 12px;
  font-size: 14px;
  width: 100%;
}

.input-group input:focus,
.input-group select:focus {
  outline: none;
  border-color: #6c63ff;
}

/* Radio group */
.radio-group {
  display: flex;
  flex-direction: column;
  gap: 8px;
  width: 100%;
  text-align: left;
}

.radio-group label {
  font-size: 14px;
  cursor: pointer;
  display: flex;
  align-items: center;
  gap: 8px;
}

/* Buttons */
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

button:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

button.secondary {
  background: #333;
}

button.secondary:hover {
  background: #444;
}

button.small {
  padding: 8px 16px;
  font-size: 13px;
}

.button-row {
  display: flex;
  gap: 12px;
}

/* Progress */
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

/* Checkmark */
.checkmark {
  font-size: 48px;
  color: #4caf50;
}

/* Error */
.error p {
  color: #ff6b6b;
}
</style>
