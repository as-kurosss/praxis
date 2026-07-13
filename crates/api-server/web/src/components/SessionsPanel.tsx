import { useState, useEffect, useRef } from 'react'
import * as api from '../api'
import type { SessionSummary } from '../types'

interface Props {
  sessions: SessionSummary[]
  currentSessionId: string | null
  onSelectSession: (sessionId: string) => void
  onNewSession: () => void
  agentId: string
  onSessionsChange: () => void
  addToast?: (msg: string, type?: 'error' | 'success') => void
}

function getDateGroup(dateStr: string): string {
  const d = new Date(dateStr)
  const now = new Date()
  const diffDays = Math.floor((now.getTime() - d.getTime()) / 86400000)
  if (diffDays === 0) return 'Today'
  if (diffDays === 1) return 'Yesterday'
  if (diffDays < 7) return 'This Week'
  if (diffDays < 30) return 'This Month'
  return 'Older'
}

export function SessionsPanel({
  sessions, currentSessionId, onSelectSession, onNewSession, agentId, onSessionsChange, addToast,
}: Props) {
  const [open, setOpen] = useState(false)
  const [filter, setFilter] = useState('')
  const [contextMenu, setContextMenu] = useState<{ x: number; y: number; session: SessionSummary } | null>(null)
  const [renaming, setRenaming] = useState<string | null>(null)
  const [renameValue, setRenameValue] = useState('')
  const ref = useRef<HTMLDivElement>(null)
  const renameRef = useRef<HTMLInputElement>(null)

  // Close on click outside
  useEffect(() => {
    const handler = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) {
        setOpen(false)
        setContextMenu(null)
      }
    }
    if (open) document.addEventListener('mousedown', handler)
    return () => document.removeEventListener('mousedown', handler)
  }, [open])

  useEffect(() => {
    if (renaming && renameRef.current) renameRef.current.focus()
  }, [renaming])

  const filteredSessions = filter
    ? sessions.filter(s => (s.title || '').toLowerCase().includes(filter.toLowerCase()))
    : sessions

  // Group by date
  const grouped = filteredSessions.reduce<Record<string, SessionSummary[]>>((acc, s) => {
    const group = getDateGroup(s.updated_at)
    if (!acc[group]) acc[group] = []
    acc[group].push(s)
    return acc
  }, {})
  const groupOrder = ['Today', 'Yesterday', 'This Week', 'This Month', 'Older']

  const handleContextMenu = (e: React.MouseEvent, s: SessionSummary) => {
    e.preventDefault()
    setContextMenu({ x: e.clientX, y: e.clientY, session: s })
  }

  const renameSession = async (id: string, title: string) => {
    try {
      await api.updateSessionTitle(id, title)
      onSessionsChange()
      addToast?.('Session renamed', 'success')
    } catch { /* ignore */ }
    setRenaming(null)
  }

  const deleteSession = async (id: string) => {
    if (!confirm('Delete this session?')) return
    try {
      await api.deleteSession(id)
      onSessionsChange()
      addToast?.('Session deleted', 'success')
    } catch { /* ignore */ }
    setContextMenu(null)
  }

  return (
    <div ref={ref} style={{ position: 'relative' }}>
      <button className="btn btn-outline btn-sm" onClick={() => setOpen(!open)}>
        {open ? '▲' : '▼'} Sessions {sessions.length > 0 && `(${sessions.length})`}
      </button>
      <button className="btn btn-outline btn-sm" onClick={onNewSession} style={{ marginLeft: 4 }}>
        + New
      </button>

      {open && (
        <div style={{
          position: 'absolute', top: '100%', right: 0, marginTop: 4,
          width: 300, maxHeight: 360, overflowY: 'auto',
          background: 'var(--surface2)', border: '1px solid var(--border)',
          borderRadius: 'var(--radius)', boxShadow: 'var(--shadow)', zIndex: 50,
          display: 'flex', flexDirection: 'column',
        }}>
          {/* Search filter */}
          <div style={{ padding: '6px 8px', borderBottom: '1px solid var(--border)' }}>
            <input value={filter} onChange={e => setFilter(e.target.value)}
              placeholder="Filter by title..."
              style={{
                width: '100%', padding: '4px 8px', fontSize: 11,
                border: '1px solid var(--border)', borderRadius: 'var(--radius-sm)',
                background: 'var(--bg)', color: 'var(--text)', outline: 'none',
              }} />
          </div>

          <div style={{ flex: 1, overflowY: 'auto' }}>
            {filteredSessions.length === 0 ? (
              <div style={{ padding: 16, color: 'var(--text2)', fontSize: 12, textAlign: 'center' }}>
                {filter ? 'No matching sessions' : 'No sessions yet'}
              </div>
            ) : (
              groupOrder.map(group => {
                const items = grouped[group]
                if (!items?.length) return null
                return (
                  <div key={group}>
                    <div style={{
                      padding: '6px 12px', fontSize: 10, fontWeight: 600,
                      color: 'var(--text2)', textTransform: 'uppercase',
                      letterSpacing: '0.5px', background: 'rgba(0,0,0,.15)',
                    }}>{group}</div>
                    {items.map(s => (
                      <div key={s.id}
                        onClick={() => { onSelectSession(s.id); setOpen(false) }}
                        onContextMenu={e => handleContextMenu(e, s)}
                        style={{
                          padding: '8px 12px', cursor: 'pointer', fontSize: 12,
                          borderBottom: '1px solid var(--border)',
                          background: s.id === currentSessionId ? 'var(--accent-dim)' : 'transparent',
                          transition: 'background .1s',
                        }}
                        onMouseEnter={e => { if (s.id !== currentSessionId) (e.target as HTMLElement).style.background = 'var(--surface)' }}
                        onMouseLeave={e => { if (s.id !== currentSessionId) (e.target as HTMLElement).style.background = 'transparent' }}
                      >
                        {renaming === s.id ? (
                          <input ref={renameRef} value={renameValue}
                            onChange={e => setRenameValue(e.target.value)}
                            onKeyDown={e => {
                              if (e.key === 'Enter') renameSession(s.id, renameValue)
                              if (e.key === 'Escape') setRenaming(null)
                            }}
                            onBlur={() => setRenaming(null)}
                            style={{ width: '100%', fontSize: 12, padding: '2px 4px' }}
                            onClick={e => e.stopPropagation()}
                          />
                        ) : (
                          <div style={{ fontWeight: 500, marginBottom: 2 }}>
                            {s.title || `Session ${s.id.slice(0, 8)}`}
                          </div>
                        )}
                        <div style={{ color: 'var(--text2)', fontSize: 11 }}>
                          {s.message_count} messages
                        </div>
                        {s.preview.length > 0 && (
                          <div style={{ color: 'var(--text2)', fontSize: 10, marginTop: 2, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                            {s.preview[0]}
                          </div>
                        )}
                      </div>
                    ))}
                  </div>
                )
              })
            )}
          </div>
        </div>
      )}

      {/* Right-click context menu */}
      {contextMenu && (
        <div style={{
          position: 'fixed', left: contextMenu.x, top: contextMenu.y,
          background: 'var(--surface)', border: '1px solid var(--border)',
          borderRadius: 'var(--radius)', boxShadow: 'var(--shadow)', zIndex: 100,
          minWidth: 140, overflow: 'hidden',
        }}>
          <div style={{ padding: '8px 12px', cursor: 'pointer', fontSize: 12,
            borderBottom: '1px solid var(--border)', transition: 'background .1s',
          }}
            onMouseEnter={e => (e.target as HTMLElement).style.background = 'var(--surface2)'}
            onMouseLeave={e => (e.target as HTMLElement).style.background = 'transparent'}
            onClick={() => {
              setRenaming(contextMenu.session.id)
              setRenameValue(contextMenu.session.title || `Session ${contextMenu.session.id.slice(0, 8)}`)
              setContextMenu(null)
            }}>✎ Rename</div>
          <div style={{ padding: '8px 12px', cursor: 'pointer', fontSize: 12,
            borderBottom: '1px solid var(--border)',
          }}
            onMouseEnter={e => (e.target as HTMLElement).style.background = 'var(--surface2)'}
            onMouseLeave={e => (e.target as HTMLElement).style.background = 'transparent'}
            onClick={() => {
              addToast?.('Session pinned', 'success')
              setContextMenu(null)
            }}>📌 Pin</div>
          <div style={{ padding: '8px 12px', cursor: 'pointer', fontSize: 12, color: 'var(--red)' }}
            onMouseEnter={e => (e.target as HTMLElement).style.background = 'var(--surface2)'}
            onMouseLeave={e => (e.target as HTMLElement).style.background = 'transparent'}
            onClick={() => deleteSession(contextMenu.session.id)}>✕ Delete</div>
        </div>
      )}
    </div>
  )
}
