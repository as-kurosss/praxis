import { useState, useEffect, useRef } from 'react'
import type { SessionSummary } from '../types'

interface Props {
  sessions: SessionSummary[]
  currentSessionId: string | null
  onSelectSession: (sessionId: string) => void
  onNewSession: () => void
  agentId: string
  onSessionsChange: () => void
}

export function SessionsPanel({
  sessions, currentSessionId, onSelectSession, onNewSession, agentId, onSessionsChange,
}: Props) {
  const [open, setOpen] = useState(false)
  const ref = useRef<HTMLDivElement>(null)

  // Close on click outside
  useEffect(() => {
    const handler = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false)
    }
    if (open) document.addEventListener('mousedown', handler)
    return () => document.removeEventListener('mousedown', handler)
  }, [open])

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
          width: 280, maxHeight: 300, overflowY: 'auto',
          background: 'var(--surface2)', border: '1px solid var(--border)',
          borderRadius: 'var(--radius)', boxShadow: 'var(--shadow)', zIndex: 50,
        }}>
          {sessions.length === 0 ? (
            <div style={{ padding: 16, color: 'var(--text2)', fontSize: 12, textAlign: 'center' }}>
              No sessions yet
            </div>
          ) : (
            sessions.map(s => (
              <div key={s.id}
                onClick={() => { onSelectSession(s.id); setOpen(false) }}
                style={{
                  padding: '8px 12px', cursor: 'pointer', fontSize: 12,
                  borderBottom: '1px solid var(--border)',
                  background: s.id === currentSessionId ? 'var(--accent-dim)' : 'transparent',
                  transition: 'background .1s',
                }}
                onMouseEnter={e => { if (s.id !== currentSessionId) (e.target as HTMLElement).style.background = 'var(--surface)' }}
                onMouseLeave={e => { if (s.id !== currentSessionId) (e.target as HTMLElement).style.background = 'transparent' }}
              >
                <div style={{ fontWeight: 500, marginBottom: 2 }}>
                  {s.title || `Session ${s.id.slice(0, 8)}`}
                </div>
                <div style={{ color: 'var(--text2)', fontSize: 11 }}>
                  {s.message_count} messages · {s.updated_at}
                </div>
                {s.preview.length > 0 && (
                  <div style={{ color: 'var(--text2)', fontSize: 10, marginTop: 2, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                    {s.preview[0]}
                  </div>
                )}
              </div>
            ))
          )}
        </div>
      )}
    </div>
  )
}
