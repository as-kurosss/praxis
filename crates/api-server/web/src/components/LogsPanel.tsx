import { useState, useEffect, useRef } from 'react'
import * as api from '../api'
import type { LogEntry } from '../types'

interface Props {
  addToast: (msg: string, type?: 'error' | 'success') => void
}

export function LogsPanel({ addToast }: Props) {
  const [logs, setLogs] = useState<LogEntry[]>([])
  const [loading, setLoading] = useState(true)
  const [autoScroll, setAutoScroll] = useState(true)
  const [filter, setFilter] = useState<string>('all')
  const containerRef = useRef<HTMLDivElement>(null)
  const pollingRef = useRef<ReturnType<typeof setInterval> | null>(null)

  const load = async () => {
    try { setLogs(await api.streamLogs()) }
    catch (e: any) { addToast(e.message) }
    finally { setLoading(false) }
  }

  // Initial load
  useEffect(() => { load() }, [])

  // Poll for new logs every 3 seconds
  useEffect(() => {
    pollingRef.current = setInterval(async () => {
      try { setLogs(await api.streamLogs()) }
      catch { /* silent poll failure */ }
    }, 3000)
    return () => { if (pollingRef.current) clearInterval(pollingRef.current) }
  }, [])

  // Auto-scroll
  useEffect(() => {
    if (autoScroll && containerRef.current) {
      containerRef.current.scrollTop = containerRef.current.scrollHeight
    }
  }, [logs, autoScroll])

  const levelColor = (level: string) => {
    switch (level) {
      case 'error': return 'var(--red)'
      case 'warn': return '#d97706'
      case 'info': return 'var(--accent)'
      case 'debug': return 'var(--text2)'
      case 'trace': return 'var(--text2)'
      default: return 'var(--text)'
    }
  }

  const filteredLogs = filter === 'all'
    ? logs
    : logs.filter(l => l.level === filter)

  if (loading) return <div className="empty-state"><p>Loading logs...</p></div>

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%' }}>
      {/* Controls */}
      <div className="flex-between" style={{ marginBottom: 8 }}>
        <div style={{ display: 'flex', gap: 4 }}>
          {['all', 'error', 'warn', 'info', 'debug', 'trace'].map(l => (
            <button key={l}
              className={`btn btn-sm ${filter === l ? 'btn-primary' : 'btn-ghost'}`}
              onClick={() => setFilter(l)}
              style={{ fontSize: 10, padding: '2px 6px' }}>
              {l.toUpperCase()}
            </button>
          ))}
        </div>
        <label style={{ display: 'flex', alignItems: 'center', gap: 4, fontSize: 11, cursor: 'pointer' }}>
          <input type="checkbox" checked={autoScroll} onChange={e => setAutoScroll(e.target.checked)} />
          Auto-scroll
        </label>
      </div>

      {/* Log container */}
      <div ref={containerRef} style={{
        flex: 1, overflowY: 'auto', fontFamily: 'monospace', fontSize: 11,
        background: 'rgba(0,0,0,.3)', borderRadius: 'var(--radius)',
        padding: 8, minHeight: 200,
      }}>
        {filteredLogs.length === 0 ? (
          <div style={{ color: 'var(--text2)', textAlign: 'center', padding: 20 }}>No log entries.</div>
        ) : (
          filteredLogs.map((log, i) => (
            <div key={i} style={{
              padding: '2px 0', display: 'flex', gap: 8,
              borderBottom: '1px solid rgba(255,255,255,.03)',
            }}>
              <span style={{ color: 'var(--text2)', flexShrink: 0 }}>{log.timestamp?.split('T')[1]?.split('.')[0] || '--:--:--'}</span>
              <span style={{ color: levelColor(log.level), flexShrink: 0, width: 40 }}>{log.level.toUpperCase()}</span>
              <span style={{ color: 'var(--text2)', flexShrink: 0, marginRight: 4 }}>[{log.target}]</span>
              <span style={{ wordBreak: 'break-word' }}>{log.message}</span>
            </div>
          ))
        )}
      </div>

      <div style={{ fontSize: 10, color: 'var(--text2)', marginTop: 4 }}>
        {logs.length} entries · Polling every 3s
      </div>
    </div>
  )
}
