import { useState, useRef, useEffect, useCallback } from 'react'
import { chatNonStreaming, getSession } from '../api'
import type { ChatMessage } from '../types'

// Simple syntax highlighting for code blocks
function highlightCode(lang: string, code: string): string {
  // Basic keyword highlighting for common languages
  const escape = (s: string) => s.replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;')
  const escaped = escape(code)
  if (lang === 'json' || lang === 'javascript' || lang === 'typescript' || lang === 'js' || lang === 'ts') {
    return escaped.replace(
      /("(?:[^"\\]|\\.)*")|(\b(?:const|let|var|function|return|import|export|if|else|async|await|true|false|null|undefined)\b)|(\/\/.*$)/gm,
      (_, str, kw, comment) =>
        str ? `<span style="color:#ce9178">${str}</span>`
        : kw ? `<span style="color:#569cd6">${kw}</span>`
        : comment ? `<span style="color:#6a9955">${comment}</span>`
        : _
    )
  }
  if (lang === 'python' || lang === 'py') {
    return escaped.replace(
      /("(?:[^"\\]|\\.)*")|('(?:[^'\\]|\\.)*')|(\b(?:def|class|import|from|return|if|else|elif|for|while|True|False|None|async|await)\b)|(#.*$)/gm,
      (_, dq, sq, kw, comment) =>
        dq ? `<span style="color:#ce9178">${dq}</span>`
        : sq ? `<span style="color:#ce9178">${sq}</span>`
        : kw ? `<span style="color:#569cd6">${kw}</span>`
        : comment ? `<span style="color:#6a9955">${comment}</span>`
        : _
    )
  }
  if (lang === 'bash' || lang === 'sh' || lang === 'shell') {
    return escaped.replace(
      /(#.*$)|("(?:[^"\\]|\\.)*")/gm,
      (_, comment, str) =>
        comment ? `<span style="color:#6a9955">${comment}</span>`
        : str ? `<span style="color:#ce9178">${str}</span>`
        : _
    )
  }
  return escaped
}

function formatMessageContent(content: string): string {
  // Replace code blocks with syntax-highlighted HTML
  return content.replace(
    /```(\w*)\n([\s\S]*?)```/g,
    (_, lang, code) => {
      const highlighted = highlightCode(lang, code)
      return `<div class="code-block"><div class="code-header">${lang || 'code'}</div><pre><code>${highlighted}</code></pre></div>`
    }
  )
}

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
  const [tokenUsage, setTokenUsage] = useState<{ prompt?: number; completion?: number; total?: number } | null>(null)
  const [showTokenPopover, setShowTokenPopover] = useState(false)
  const [approvalQueue, setApprovalQueue] = useState<{ id: string; tool: string; args: string }[]>([])
  const [showSlashMenu, setShowSlashMenu] = useState(false)
  const chatRef = useRef<HTMLDivElement>(null)
  const inputRef = useRef<HTMLInputElement>(null)
  const eventSourceRef = useRef<EventSource | null>(null)

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

  const handleSlashCommand = useCallback((cmd: string) => {
    switch (cmd) {
      case '/clear':
        onMessagesChange([])
        setTokenUsage(null)
        break
      case '/compact':
        addToast('Compacting conversation...', 'success')
        break
      case '/approve':
        if (approvalQueue.length > 0) {
          const next = approvalQueue[0]
          addToast(`Approved: ${next.tool}`, 'success')
          setApprovalQueue(prev => prev.slice(1))
        }
        break
    }
  }, [onMessagesChange, addToast, approvalQueue])

  const sendMessage = useCallback(async () => {
    const text = input.trim()
    if (!text || streaming) return

    // Check for slash commands
    if (text.startsWith('/')) {
      setInput('')
      handleSlashCommand(text.toLowerCase())
      return
    }

    setInput('')
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
      msgs[assistantIndex] = {
        role: 'assistant',
        content: assistantContent,
        reasoning_content: reasoningContent || null,
        tool_calls: toolCalls.length > 0 ? toolCalls.map(tc => ({ id: tc.id, name: tc.name, arguments: null })) : null,
      }
      onMessagesChange(msgs)
      // Only tell parent about session id on success
      if (saveSession && currentSession) onSessionChange(currentSession)
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
  }, [input, streaming, messages, agentId, sessionId, onMessagesChange, onSessionChange, addToast])

  const fallbackToNonStreaming = async (text: string, sid: string, currentMessages: ChatMessage[]) => {
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
                  {msg.content ? (
                    <div dangerouslySetInnerHTML={{ __html: formatMessageContent(msg.content) }} />
                  ) : ''}
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
                  {/* Approval cards for tool calls that need approval */}
                  {msg.tool_calls && msg.tool_calls.length > 0 && (
                    <div className="approval-area">
                      {msg.tool_calls.map((tc, j) => (
                        <div key={j} className="approval-card">
                          <div className="approval-card-header">
                            <span>🔧 {tc.name}</span>
                            <span className="badge badge-ollama">Requires Approval</span>
                          </div>
                          <div className="approval-card-args">
                            {tc.arguments ? JSON.stringify(tc.arguments, null, 2) : 'No arguments'}
                          </div>
                          <div className="approval-card-actions">
                            <button className="btn btn-primary btn-sm"
                              onClick={() => addToast('Tool approved', 'success')}>Approve</button>
                            <button className="btn btn-danger btn-sm"
                              onClick={() => addToast('Tool denied', 'error')}>Deny</button>
                          </div>
                        </div>
                      ))}
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

      {/* Token usage popover */}
      {tokenUsage && (
        <div style={{ position: 'relative' }}>
          <button className="btn btn-ghost btn-sm"
            onClick={() => setShowTokenPopover(!showTokenPopover)}
            style={{ position: 'absolute', bottom: 60, right: 16, fontSize: 11, zIndex: 10 }}>
            Token Usage
          </button>
          {showTokenPopover && (
            <div style={{
              position: 'absolute', bottom: 80, right: 16,
              background: 'var(--surface2)', border: '1px solid var(--border)',
              borderRadius: 'var(--radius)', padding: 12, zIndex: 20,
              minWidth: 200, boxShadow: 'var(--shadow)',
            }}>
              <h4 style={{ fontSize: 12, marginBottom: 8 }}>Token Usage</h4>
              <div style={{ fontSize: 11 }}>
                {tokenUsage.prompt != null && (
                  <div className="flex-between" style={{ padding: '2px 0' }}>
                    <span style={{ color: 'var(--text2)' }}>Prompt:</span>
                    <span>{tokenUsage.prompt.toLocaleString()}</span>
                  </div>
                )}
                {tokenUsage.completion != null && (
                  <div className="flex-between" style={{ padding: '2px 0' }}>
                    <span style={{ color: 'var(--text2)' }}>Completion:</span>
                    <span>{tokenUsage.completion.toLocaleString()}</span>
                  </div>
                )}
                {tokenUsage.total != null && (
                  <div className="flex-between" style={{ padding: '2px 0', fontWeight: 600 }}>
                    <span>Total:</span>
                    <span>{tokenUsage.total.toLocaleString()}</span>
                  </div>
                )}
              </div>
            </div>
          )}
        </div>
      )}

      <div className="chat-input-area">
        <div style={{ flex: 1, position: 'relative' }}>
          <input
            ref={inputRef}
            value={input}
            onChange={e => {
              setInput(e.target.value)
              setShowSlashMenu(e.target.value === '/')
            }}
            onKeyDown={handleKeyDown}
            placeholder={streaming ? 'Waiting for response...' : "Type a message... (type / for commands)"}
            disabled={streaming}
          />
          {/* Slash commands menu */}
          {showSlashMenu && (
            <div style={{
              position: 'absolute', bottom: '100%', left: 0, marginBottom: 4,
              background: 'var(--surface2)', border: '1px solid var(--border)',
              borderRadius: 'var(--radius)', boxShadow: 'var(--shadow)', zIndex: 20,
              minWidth: 180, overflow: 'hidden',
            }}>
              {[
                { cmd: '/clear', desc: 'Clear conversation' },
                { cmd: '/compact', desc: 'Compact conversation history' },
                { cmd: '/approve', desc: 'Approve pending tool call' },
              ].map(s => (
                <div key={s.cmd}
                  onClick={() => {
                    setInput(s.cmd + ' ')
                    setShowSlashMenu(false)
                    inputRef.current?.focus()
                  }}
                  style={{
                    padding: '8px 12px', cursor: 'pointer', fontSize: 12,
                    borderBottom: '1px solid var(--border)', transition: 'background .1s',
                  }}
                  onMouseEnter={e => (e.target as HTMLElement).style.background = 'var(--surface)'}
                  onMouseLeave={e => (e.target as HTMLElement).style.background = 'transparent'}
                >
                  <strong>{s.cmd}</strong>
                  <span style={{ color: 'var(--text2)', marginLeft: 8 }}>{s.desc}</span>
                </div>
              ))}
            </div>
          )}
        </div>
        <button className="btn btn-primary" onClick={sendMessage} disabled={!input.trim() || streaming}>
          {streaming ? '...' : 'Send'}
        </button>
      </div>
    </>
  )
}
