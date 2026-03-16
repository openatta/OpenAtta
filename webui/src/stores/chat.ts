import { defineStore } from 'pinia'
import { ref } from 'vue'

export interface ChatMessage {
  id: string
  role: 'user' | 'assistant' | 'tool' | 'system'
  content: string
  tool_name?: string
  call_id?: string
  duration_ms?: number
  status?: 'running' | 'completed' | 'error'
  timestamp: number
}

export const useChatStore = defineStore('chat', () => {
  const messages = ref<ChatMessage[]>([])
  const streaming = ref(false)
  const error = ref<string | null>(null)

  let abortController: AbortController | null = null

  function addUserMessage(content: string) {
    messages.value.push({
      id: crypto.randomUUID(),
      role: 'user',
      content,
      timestamp: Date.now(),
    })
  }

  async function sendMessage(content: string) {
    addUserMessage(content)
    streaming.value = true
    error.value = null

    abortController = new AbortController()

    // Create a placeholder assistant message
    const assistantId = crypto.randomUUID()
    messages.value.push({
      id: assistantId,
      role: 'assistant',
      content: '',
      timestamp: Date.now(),
    })

    try {
      const response = await fetch('/api/v1/chat', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ message: content }),
        signal: abortController.signal,
      })

      if (!response.ok) {
        throw new Error(`HTTP ${response.status}: ${response.statusText}`)
      }

      const reader = response.body?.getReader()
      if (!reader) throw new Error('No response body')

      const decoder = new TextDecoder()
      let buffer = ''

      while (true) {
        const { done, value } = await reader.read()
        if (done) break

        buffer += decoder.decode(value, { stream: true })
        const lines = buffer.split('\n')
        buffer = lines.pop() || ''

        for (const line of lines) {
          if (!line.startsWith('data: ')) continue
          const raw = line.slice(6).trim()
          if (!raw) continue

          try {
            const event = JSON.parse(raw)
            handleSSEEvent(event, assistantId)
          } catch {
            // skip malformed events
          }
        }
      }
    } catch (e: any) {
      if (e.name !== 'AbortError') {
        error.value = e.message || 'Failed to send message'
        const msg = messages.value.find(m => m.id === assistantId)
        if (msg && !msg.content) {
          msg.content = `Error: ${error.value}`
        }
      }
    } finally {
      streaming.value = false
      abortController = null
    }
  }

  function handleSSEEvent(event: any, assistantId: string) {
    const msg = messages.value.find(m => m.id === assistantId)
    if (!msg) return

    // Backend uses serde(tag="type", content="data"), so fields are in event.data
    const data = event.data || {}
    const type = event.type || ''

    switch (type) {
      case 'thinking':
        // Show thinking indicator as a system message
        messages.value.push({
          id: crypto.randomUUID(),
          role: 'system',
          content: `🔄 Thinking... (iteration ${data.iteration || 1})`,
          timestamp: Date.now(),
        })
        break

      case 'text_delta':
        msg.content += data.delta || ''
        break

      case 'tool_start':
        messages.value.push({
          id: crypto.randomUUID(),
          role: 'tool',
          content: '',
          tool_name: data.tool_name,
          call_id: data.call_id,
          status: 'running',
          timestamp: Date.now(),
        })
        break

      case 'tool_complete': {
        const toolMsg = messages.value.find(
          m => m.role === 'tool' && m.call_id === data.call_id
        )
        if (toolMsg) {
          toolMsg.status = 'completed'
          toolMsg.duration_ms = data.duration_ms
        }
        break
      }

      case 'tool_error': {
        const errMsg = messages.value.find(
          m => m.role === 'tool' && m.call_id === data.call_id
        )
        if (errMsg) {
          errMsg.status = 'error'
          errMsg.content = data.error || 'Unknown error'
        }
        break
      }

      case 'error':
        error.value = data.message || 'Unknown error'
        if (!msg.content) msg.content = `Error: ${data.message}`
        break

      case 'done':
        // Remove thinking indicators
        messages.value = messages.value.filter(m => m.role !== 'system')
        break
    }
  }

  function stopStreaming() {
    abortController?.abort()
    streaming.value = false
  }

  function clearMessages() {
    messages.value = []
    error.value = null
  }

  return { messages, streaming, error, sendMessage, stopStreaming, clearMessages }
})
