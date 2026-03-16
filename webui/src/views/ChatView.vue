<script setup lang="ts">
import { ref, nextTick, watch, computed } from 'vue'
import { useChatStore } from '../stores/chat'
import { useI18n } from 'vue-i18n'

const { t } = useI18n()
const chatStore = useChatStore()
const input = ref('')
const messagesContainer = ref<HTMLElement | null>(null)

const hasMessages = computed(() => chatStore.messages.length > 0)

function handleSend() {
  const text = input.value.trim()
  if (!text || chatStore.streaming) return
  input.value = ''
  chatStore.sendMessage(text)
}

function handleKeydown(e: KeyboardEvent) {
  if (e.key === 'Enter' && !e.shiftKey) {
    e.preventDefault()
    handleSend()
  }
}

function scrollToBottom() {
  nextTick(() => {
    if (messagesContainer.value) {
      messagesContainer.value.scrollTop = messagesContainer.value.scrollHeight
    }
  })
}

watch(() => chatStore.messages.length, scrollToBottom)
watch(() => chatStore.messages[chatStore.messages.length - 1]?.content, scrollToBottom)
</script>

<template>
  <div class="chat-view">
    <!-- Empty welcome state -->
    <div v-if="!hasMessages" class="welcome">
      <div class="welcome-inner">
        <div class="welcome-logo">A</div>
        <h1 class="welcome-title">AttaOS</h1>
        <p class="welcome-sub">{{ t('chat.empty_hint') }}</p>
      </div>
    </div>

    <!-- Messages -->
    <div v-else ref="messagesContainer" class="messages-scroll">
      <div class="messages-inner">
        <template v-for="msg in chatStore.messages" :key="msg.id">

          <!-- User -->
          <div v-if="msg.role === 'user'" class="msg-row msg-row-user">
            <div class="msg-bubble msg-bubble-user">
              {{ msg.content }}
            </div>
          </div>

          <!-- Assistant -->
          <div v-else-if="msg.role === 'assistant'" class="msg-row msg-row-assistant">
            <div class="msg-avatar">A</div>
            <div class="msg-body">
              <div class="msg-text">
                {{ msg.content }}<span v-if="chatStore.streaming && msg === chatStore.messages[chatStore.messages.length - 1]" class="cursor"></span>
              </div>
            </div>
          </div>

          <!-- Thinking -->
          <div v-else-if="msg.role === 'system'" class="msg-row msg-row-system">
            <div class="thinking-pill">
              <span class="thinking-dot"></span>
              {{ msg.content }}
            </div>
          </div>

          <!-- Tool -->
          <div v-else-if="msg.role === 'tool'" class="msg-row msg-row-tool">
            <div class="tool-pill" :class="`tool-pill--${msg.status || 'running'}`">
              <span class="tool-pill-icon">
                <template v-if="msg.status === 'completed'">&#10003;</template>
                <template v-else-if="msg.status === 'error'">&#10007;</template>
                <template v-else><span class="spinner"></span></template>
              </span>
              <span class="tool-pill-name">{{ msg.tool_name || 'tool' }}</span>
              <span v-if="msg.duration_ms != null && msg.status === 'completed'" class="tool-pill-meta">{{ msg.duration_ms }}ms</span>
              <span v-if="msg.status === 'running'" class="tool-pill-meta">{{ t('common.status_running') }}</span>
            </div>
            <div v-if="msg.content && msg.status === 'error'" class="tool-error">{{ msg.content }}</div>
          </div>

        </template>
      </div>
    </div>

    <!-- Input area -->
    <div class="input-area" :class="{ 'input-area--welcome': !hasMessages }">
      <div class="input-wrap">
        <div v-if="chatStore.error" class="input-error">{{ chatStore.error }}</div>
        <div class="input-box">
          <textarea
            v-model="input"
            class="input-textarea"
            :placeholder="t('chat.placeholder')"
            rows="1"
            @keydown="handleKeydown"
            @input="($event.target as HTMLTextAreaElement).style.height = 'auto'; ($event.target as HTMLTextAreaElement).style.height = Math.min(($event.target as HTMLTextAreaElement).scrollHeight, 150) + 'px'"
          />
          <div class="input-actions">
            <button
              v-if="chatStore.streaming"
              class="btn-stop"
              @click="chatStore.stopStreaming()"
              :title="t('chat.stop')"
            >
              <svg width="16" height="16" viewBox="0 0 16 16" fill="currentColor"><rect x="3" y="3" width="10" height="10" rx="1.5"/></svg>
            </button>
            <button
              v-else
              class="btn-send"
              :class="{ active: input.trim() }"
              :disabled="!input.trim()"
              @click="handleSend"
              :title="t('chat.send')"
            >
              <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M14 2L2 8.5l4.5 2L10 6l-2.5 5.5L14 2z"/></svg>
            </button>
          </div>
        </div>
        <div class="input-footer">
          <button v-if="hasMessages" class="btn-clear" @click="chatStore.clearMessages()">{{ t('chat.clear') }}</button>
          <span class="input-hint">Enter {{ t('chat.send') }}，Shift+Enter {{ t('common.wrap') || 'wrap' }}</span>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.chat-view {
  display: flex;
  flex-direction: column;
  flex: 1;
  min-height: 0;
  overflow: hidden;
  background: var(--bg-page);
}

/* ── Welcome ── */
.welcome {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
}
.welcome-inner {
  text-align: center;
}
.welcome-logo {
  width: 48px;
  height: 48px;
  border-radius: 50%;
  background: var(--color-primary);
  color: white;
  font-size: 1.5rem;
  font-weight: 700;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  margin-bottom: 1rem;
}
.welcome-title {
  font-size: 1.5rem;
  font-weight: 600;
  color: var(--text-primary);
  margin: 0 0 0.5rem;
}
.welcome-sub {
  color: var(--text-secondary);
  font-size: 0.9375rem;
  margin: 0;
}

/* ── Messages scroll ── */
.messages-scroll {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  scroll-behavior: smooth;
}
.messages-inner {
  max-width: 720px;
  margin: 0 auto;
  padding: 1.5rem 1rem;
  display: flex;
  flex-direction: column;
  gap: 1.25rem;
}

/* ── Message rows ── */
.msg-row {
  display: flex;
}
.msg-row-user {
  justify-content: flex-end;
}
.msg-row-assistant {
  gap: 0.75rem;
  align-items: flex-start;
}
.msg-row-system {
  justify-content: center;
}
.msg-row-tool {
  flex-direction: column;
  padding-left: 2.75rem;
  gap: 0.25rem;
}

/* User bubble */
.msg-bubble-user {
  background: var(--color-primary);
  color: white;
  padding: 0.625rem 1rem;
  border-radius: 1.25rem 1.25rem 0.25rem 1.25rem;
  max-width: 80%;
  white-space: pre-wrap;
  word-break: break-word;
  line-height: 1.5;
  font-size: 0.9375rem;
}

/* Assistant */
.msg-avatar {
  width: 28px;
  height: 28px;
  border-radius: 50%;
  background: var(--color-primary);
  color: white;
  font-size: 0.75rem;
  font-weight: 700;
  display: flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
  margin-top: 2px;
}
.msg-body {
  max-width: calc(100% - 3rem);
  min-width: 0;
}
.msg-text {
  white-space: pre-wrap;
  word-break: break-word;
  line-height: 1.7;
  font-size: 0.9375rem;
  color: var(--text-primary);
}

/* Cursor */
.cursor {
  display: inline-block;
  width: 7px;
  height: 1.1em;
  background: var(--color-primary);
  margin-left: 2px;
  border-radius: 1px;
  vertical-align: text-bottom;
  animation: pulse 1s ease-in-out infinite;
}
@keyframes pulse {
  0%, 100% { opacity: 1; }
  50% { opacity: 0.3; }
}

/* Thinking */
.thinking-pill {
  display: inline-flex;
  align-items: center;
  gap: 0.5rem;
  padding: 0.25rem 0.75rem;
  border-radius: 999px;
  background: var(--bg-surface);
  color: var(--text-secondary);
  font-size: 0.8125rem;
  border: 1px solid var(--border-color);
}
.thinking-dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--color-primary);
  animation: pulse 1s ease-in-out infinite;
}

/* Tool pill */
.tool-pill {
  display: inline-flex;
  align-items: center;
  gap: 0.375rem;
  padding: 0.25rem 0.625rem;
  border-radius: 6px;
  font-size: 0.8125rem;
  background: var(--bg-surface);
  border: 1px solid var(--border-color);
  color: var(--text-secondary);
  font-family: var(--font-mono, monospace);
}
.tool-pill--completed {
  border-color: color-mix(in srgb, var(--color-success) 40%, transparent);
}
.tool-pill--error {
  border-color: color-mix(in srgb, var(--color-error) 40%, transparent);
}
.tool-pill-icon {
  font-size: 0.75rem;
  font-weight: 700;
  line-height: 1;
}
.tool-pill--completed .tool-pill-icon { color: var(--color-success); }
.tool-pill--error .tool-pill-icon { color: var(--color-error); }
.tool-pill--running .tool-pill-icon { color: var(--color-primary); }
.tool-pill-name {
  font-weight: 600;
  color: var(--text-primary);
}
.tool-pill-meta {
  font-size: 0.6875rem;
  color: var(--text-secondary);
}
.spinner {
  display: inline-block;
  width: 10px;
  height: 10px;
  border: 2px solid var(--color-primary);
  border-right-color: transparent;
  border-radius: 50%;
  animation: spin 0.6s linear infinite;
}
@keyframes spin { to { transform: rotate(360deg); } }
.tool-error {
  font-size: 0.75rem;
  color: var(--color-error);
  font-family: var(--font-mono, monospace);
  padding: 0.25rem 0.625rem;
}

/* ── Input area ── */
.input-area {
  flex-shrink: 0;
  padding: 0 1rem 1rem;
}
.input-area--welcome {
  /* Center the input when in welcome state */
}
.input-wrap {
  max-width: 720px;
  margin: 0 auto;
}
.input-error {
  color: var(--color-error);
  font-size: 0.8125rem;
  padding: 0.375rem 0;
}
.input-box {
  display: flex;
  align-items: flex-end;
  background: var(--bg-surface);
  border: 1px solid var(--border-color);
  border-radius: 1.25rem;
  padding: 0.5rem 0.5rem 0.5rem 1rem;
  transition: border-color 0.15s;
  box-shadow: 0 1px 6px rgba(0,0,0,0.06);
}
.input-box:focus-within {
  border-color: var(--color-primary);
  box-shadow: 0 1px 8px rgba(0,0,0,0.1);
}
.input-textarea {
  flex: 1;
  border: none;
  outline: none;
  resize: none;
  font-family: inherit;
  font-size: 0.9375rem;
  line-height: 1.5;
  max-height: 150px;
  background: transparent;
  color: var(--text-primary);
  padding: 0.25rem 0;
}
.input-textarea::placeholder {
  color: var(--text-secondary);
}
.input-actions {
  display: flex;
  align-items: center;
  margin-left: 0.375rem;
}
.btn-send,
.btn-stop {
  width: 32px;
  height: 32px;
  border-radius: 50%;
  border: none;
  display: flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
  transition: background 0.15s, opacity 0.15s;
}
.btn-send {
  background: var(--border-color);
  color: var(--text-secondary);
}
.btn-send.active {
  background: var(--color-primary);
  color: white;
}
.btn-send:disabled {
  cursor: default;
  opacity: 0.5;
}
.btn-stop {
  background: var(--color-error);
  color: white;
}
.btn-stop:hover {
  opacity: 0.85;
}

/* Footer */
.input-footer {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding-top: 0.375rem;
}
.input-hint {
  font-size: 0.6875rem;
  color: var(--text-secondary);
  margin-left: auto;
}
.btn-clear {
  background: none;
  border: none;
  color: var(--text-secondary);
  font-size: 0.75rem;
  cursor: pointer;
  padding: 0;
}
.btn-clear:hover {
  color: var(--text-primary);
}
</style>
