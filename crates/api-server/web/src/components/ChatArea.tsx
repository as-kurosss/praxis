import { useState, useRef, useEffect, useCallback } from 'react'
import { chatNonStreaming, getSession } from '../api'
import type { ChatMessage } from '../types'

interface Props {
  agentId: string
  sessionId: string | null
  messages: ChatMessage[]
  onMessagesChange: (msgs: ChatMessage[]) => void
  onSessionChange: (sessionId: string) => void
  addToast: (msg: string, type?: 'error' | 'success') => void
}

export function ChatArea({ agentId, sessionId, messages, onMessagesChange, onSessionChange, addToast }: Props) {
  const [input, setInput] = useState('')
  const [streaming, setStreaming] = useState(false)
  const [isLoading, setIsLoading] = useState(false)
  const [expandedReasoning, setExpandedReasoning] = useState<number | null>(null)
  const [messageQueue, setMessageQueue] = useState<string[]>([])
  const chatRef = useRef<HTMLDivElement>(null)
  const inputRef = useRef<HTMLInputElement>(null)
  const eventSourceRef = useRef<EventSource | null>(null)
  const processingRef = useRef(false)
  const queueRef = useRef<string[]>([])

  // Cleanup EventSource on unmount
  useEffect(() => {
    return () => {
      eventSourceRef.current?.close()
      eventSourceRef.current = null
    }
  }, [])

  // Auto-scroll
  useEffect(() => {
    if (chatRef.current) chatRef.current.scrollTop = chatRef.current.scrollHeight
  }, [messages])

  // Load session messages from server ONLY when switching to an existing session
  // (messages are empty on switch).  DO NOT reload when sessionId changes due to
  // onSessionChange mid-conversation — the server may not have saved yet,
  // causing stale data to overwrite the current conversation.
  useEffect(() => {
    if (sessionId && messages.length === 0) {
      getSession(sessionId).then(s => {
        onMessagesChange(s.messages)
      }).catch(() => {})
    }
  }, [sessionId]) // eslint-disable-line

  // Focus input
  useEffect(() => {
    if (!streaming) inputRef.current?.focus()
  }, [streaming])

  // Queue a message to be sent after current stream finishes, or send immediately
  const sendMessage = useCallback(async (textOverride?: string) => {
    const text = (textOverride ?? input).trim()
    if (!text) return

    if (streaming || processingRef.current) {
      // Stream in progress — queue this message
      queueRef.current = [...queueRef.current, text]
      setMessageQueue([...queueRef.current])
      setInput('')
      addToast('Message queued — will send after current response completes', 'info')
      return
    }

    setInput('')
    await doSendMessage(text)
  }, [input, streaming, addToast])

  const processNextInQueue = useCallback(() => {
    processingRef.current = false
    queueRef.current = queueRef.current.slice(1)
    setMessageQueue([...queueRef.current])
    if (queueRef.current.length > 0) {
      const nextText = queueRef.current[0]
      processingRef.current = true
      doSendMessage(nextText)
    }
  }, []) // eslint-disable-line

  const doSendMessage = useCallback(async (text: string) => {
    processingRef.current = true
    setIsLoading(true)

    // Add user message
    const userMsg: ChatMessage = { role: 'user', content: text }
    const updatedMessages = [...messages, userMsg]
    onMessagesChange(updatedMessages)

    // Try streaming first
    const streamUrl = `/api/agents/${agentId}/chat/stream?message=${encodeURIComponent(text)}${sessionId ? `&session_id=${encodeURIComponent(sessionId)}` : ''}`

    // Close any previous EventSource (safety)
    eventSourceRef.current?.close()
    const es = new EventSource(streamUrl)
    eventSourceRef.current = es
    let currentSession = sessionId || ''
    let assistantContent = ''
    let reasoningContent = ''
    let toolCalls: { id: string; name: string; }[] = []
    let done = false

    // Add a placeholder for the assistant response
    const assistantIndex = updatedMessages.length
    const placeholderMsg: ChatMessage = { role: 'assistant', content: '' }
    onMessagesChange([...updatedMessages, placeholderMsg])
    setStreaming(true)
    setIsLoading(false)

    es.addEventListener('token', (e: MessageEvent) => {
      assistantContent += e.data
      const msgs = [...updatedMessages]
      msgs[assistantIndex] = { role: 'assistant', content: assistantContent, tool_calls: toolCalls.length > 0 ? toolCalls.map(tc => ({ id: tc.id, name: tc.name, arguments: null })) : null }
      onMessagesChange(msgs)
    })

    es.addEventListener('tool_call_start', (e: MessageEvent) => {
      try {
        const data = JSON.parse(e.data)
        toolCalls = [...toolCalls, { id: data.id, name: data.name }]
        const msgs = [...updatedMessages]
        msgs[assistantIndex] = {
          role: 'assistant',
          content: assistantContent,
          tool_calls: toolCalls.map(tc => ({ id: tc.id, name: tc.name, arguments: null })),
        }
        onMessagesChange(msgs)
      } catch { /* ignore parse errors */ }
    })

    es.addEventListener('tool_call_end', () => {
      // Tool call completed — the next tokens will follow
    })

    es.addEventListener('reasoning', (e: MessageEvent) => {
      reasoningContent += e.data
      // Update the assistant message in-place with partial reasoning content
      const msgs = [...updatedMessages]
      msgs[assistantIndex] = {
        ...msgs[assistantIndex],
        reasoning_content: reasoningContent,
      }
      onMessagesChange(msgs)
    })

    es.addEventListener('session_id', (e: MessageEvent) => {
      // Store session id but DON'T update parent yet —
      // doing so would change currentSessionId → remount ChatArea mid-stream.
      currentSession = e.data
    })

    const finishStream = (saveSession: boolean) => {
      done = true
      es.close()
      eventSourceRef.current = null
      setStreaming(false)
      // Update with final content
      const msgs = [...updatedMessages]
      const finalContent = assistantContent.trim()
      msgs[assistantIndex] = {
        role: 'assistant',
        content: finalContent || null,
        reasoning_content: reasoningContent || null,
        tool_calls: toolCalls.length > 0 ? toolCalls.map(tc => ({ id: tc.id, name: tc.name, arguments: null })) : null,
      }
      onMessagesChange(msgs)
      // Only tell parent about session id on success
      if (saveSession && currentSession) onSessionChange(currentSession)
      // Process next message in queue
      setTimeout(() => processNextInQueue(), 100)
    }

    es.addEventListener('done', () => {
      finishStream(true)
    })

    es.addEventListener('error', () => {
      if (done) return
      // Close EventSource FIRST to prevent auto-reconnect,
      // which would create a second identical request on the server.
      es.close()
      eventSourceRef.current = null
      setStreaming(false)
      // Don't finishStream/fallback immediately — the `done` event might be
      // queued behind this `error` event in the JS event loop (browsers can
      // dispatch `error` from connection-close before the `done` event from
      // the last received SSE data is dispatched).
      // Wait 1.5s for `done` to arrive; if it does, `done` handler sets UI.
      // If not, this was a genuine error and we fallback.
      setTimeout(() => {
        if (done) return
        finishStream(false)
        // Save the streaming session (may differ from prop if server assigned a new one)
        if (currentSession) onSessionChange(currentSession)
        const sid = sessionId || ''
        fallbackToNonStreaming(text, sid, updatedMessages)
      }, 1500)
    })

    // Timeout safety — if no events within 30s, fallback
    const timeoutId = setTimeout(() => {
      if (!done) {
        es.close()
        eventSourceRef.current = null
        setStreaming(false)
        // Use original prop sessionId, not streaming-created currentSession
        const sid = sessionId || ''
        fallbackToNonStreaming(text, sid, updatedMessages)
      }
    }, 30000)

    es.addEventListener('done', () => clearTimeout(timeoutId), { once: true })
  }, [messages, agentId, sessionId, onMessagesChange, onSessionChange, addToast])

  const fallbackToNonStreaming = async (text: string, sid: string, currentMessages: ChatMessage[]) => {
    processingRef.current = false
    try {
      const result = await chatNonStreaming(agentId, text, sid || null)
      // Append — DO NOT replace the last element of currentMessages, because
      // currentMessages (updatedMessages) ends with the user message, not the
      // placeholder. Replacing .length-1 would silently delete the user message.
      const finalMsgs = [...currentMessages, { role: 'assistant', content: result.message }]
      onMessagesChange(finalMsgs)
      onSessionChange(result.session_id)
    } catch (e: any) {
      addToast(e.message)
      // DON'T remove the assistant response — finishStream(false) already saved
      // the streamed content (even if partial). Calling onMessagesChange(currentMessages)
      // would erase what was already streamed and shown in the UI.
    }
    setTimeout(() => processNextInQueue(), 100)
  }

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault()
      sendMessage()
    }
  }

  return (
    <>
      <div className="chat-area" ref={chatRef}>
        {messages.length === 0 ? (
          <div className="empty-state" style={{ flex: 1, display: 'flex', flexDirection: 'column', justifyContent: 'center' }}>
            <h3>Start a conversation</h3>
            <p>Type a message below to chat with this agent.</p>
          </div>
        ) : (
          messages.map((msg, i) => {
            if (msg.role === 'user') {
              return <div key={i} className="msg msg-user">{msg.content}</div>
            } else if (msg.role === 'assistant') {
              const isLast = i === messages.length - 1
              const isStreamingAssistant = streaming && isLast
              const isReasoningExpanded = expandedReasoning === i
              return (
                <div key={i} className={`msg msg-assistant${isStreamingAssistant ? ' msg-streaming' : ''}`}>
                  {msg.reasoning_content && (
                    <div className="reasoning-block">
                      <div
                        className="reasoning-header"
                        onClick={() => setExpandedReasoning(isReasoningExpanded ? null : i)}
                      >
                        <span className="reasoning-toggle">{isReasoningExpanded ? '▼' : '▶'}</span>
                        <span>Мысли модели</span>
                      </div>
                      {isReasoningExpanded && (
                        <div className="reasoning-content">{msg.reasoning_content}</div>
                      )}
                    </div>
                  )}
                  {msg.content || ''}
                  {msg.tool_calls && msg.tool_calls.length > 0 && (
                    <div style={{ marginTop: 6, fontSize: 11, color: 'var(--text2)' }}>
                      {msg.tool_calls.map((tc, j) => (
                        <div key={j}>🔧 {tc.name} ({tc.id})</div>
                      ))}
                    </div>
                  )}
                  {isStreamingAssistant && !msg.content && !msg.tool_calls?.length && (
                    <span className="cursor">▍</span>
                  )}
                  {/* Fallback for empty assistant response after streaming completes */}
                  {!isStreamingAssistant && i === messages.length - 1 && !msg.content && !msg.tool_calls?.length && !msg.reasoning_content && (
                    <div className="fallback-msg">
                      The agent returned an empty response. Try rephrasing your message or check provider settings.
                    </div>
                  )}
                </div>
              )
            } else if (msg.role === 'system') {
              return <div key={i} className="msg msg-system">{msg.content}</div>
            } else if (msg.role === 'tool') {
              return (
                <div key={i} className="msg msg-tool">
                  <strong>{msg.name || 'tool'}</strong>
                  {msg.content && <pre>{msg.content}</pre>}
                </div>
              )
            }
            return null
          })
        )}
        {isLoading && !streaming && (
          <div className="msg msg-assistant">…</div>
        )}
      </div>

      <div className="chat-input-area">
        {messageQueue.length > 0 && (
          <div className="queue-indicator">
            {messageQueue.length} queued
          </div>
        )}
        <input
          ref={inputRef}
          value={input}
          onChange={e => setInput(e.target.value)}
          onKeyDown={handleKeyDown}
          placeholder={streaming ? 'Streaming in progress...' : 'Type a message...'}
          disabled={false}
        />
        <button className="btn btn-primary" onClick={() => sendMessage()} disabled={!input.trim()}>
          {streaming ? 'Queue' : 'Send'}
        </button>
      </div>
    </>
  )
}
