import { useState, useEffect } from 'react'
import * as api from '../api'
import type { TraceSummary } from '../types'

interface Props {
  addToast: (msg: string, type?: 'error' | 'success' | 'info') => void
}

export function ObservePanel({ addToast }: Props) {
  const [traces, setTraces] = useState<TraceSummary[]>([])
  const [loading, setLoading] = useState(true)
  const [expandedTrace, setExpandedTrace] = useState<string | null>(null)

  const load = async () => {
    setLoading(true)
    try { setTraces(await api.listTraces()) }
    catch (e: any) { addToast(e.message) }
    finally { setLoading(false) }
  }

  useEffect(() => { load() }, [])

  const totalTokens = traces.reduce((sum, t) => sum + (t.token_count || 0), 0)
  const avgDuration = 0

  if (loading) return <div className="empty-state"><p>Loading traces...</p></div>

  return (
    <div>
      {/* Summary cards */}
      <div style={{ display: 'flex', gap: 8, marginBottom: 12 }}>
        <div className="card" style={{ flex: 1, cursor: 'default', textAlign: 'center' }}>
          <div style={{ fontSize: 24, fontWeight: 700, color: 'var(--accent)' }}>{traces.length}</div>
          <small>Traces</small>
        </div>
        <div className="card" style={{ flex: 1, cursor: 'default', textAlign: 'center' }}>
          <div style={{ fontSize: 24, fontWeight: 700, color: 'var(--accent)' }}>{totalTokens}</div>
          <small>Tokens</small>
        </div>
        <div className="card" style={{ flex: 1, cursor: 'default', textAlign: 'center' }}>
          <div style={{ fontSize: 24, fontWeight: 700, color: 'var(--accent)' }}>{avgDuration}ms</div>
          <small>Avg Duration</small>
        </div>
      </div>

      {/* Token usage bar chart */}
      {traces.length > 0 && (
        <div className="card" style={{ cursor: 'default', marginBottom: 12 }}>
          <h3 style={{ fontSize: 13, fontWeight: 600, marginBottom: 8 }}>Token Usage</h3>
          <div style={{ display: 'flex', gap: 2, height: 40, alignItems: 'flex-end' }}>
            {traces.slice(-20).map((t, i) => {
              const maxTokens = Math.max(...traces.map(x => x.token_count || 0), 1)
              const height = Math.max(((t.token_count || 0) / maxTokens) * 36, 4)
              return (
                <div key={t.id} style={{
                  flex: 1, height, background: 'var(--accent)',
                  borderRadius: '2px 2px 0 0', opacity: 0.6 + (i / traces.length) * 0.4,
                  minWidth: 4, position: 'relative',
                }} title={`${t.token_count} tokens`} />
              )
            })}
          </div>
        </div>
      )}

      {/* Traces list */}
      <h3 style={{ fontSize: 13, fontWeight: 600, marginBottom: 8, color: 'var(--accent)' }}>
        Recent Traces
      </h3>

      {traces.length === 0 ? (
        <div className="empty-state"><p>No traces yet. Start chatting to generate traces.</p></div>
      ) : (
        traces.map(t => (
          <div key={t.id} className="card">
            <div className="flex-between" onClick={() => setExpandedTrace(expandedTrace === t.id ? null : t.id)}
              style={{ cursor: 'pointer' }}>
              <div>
                <h3>{t.id.slice(0, 12)}...</h3>
                <p style={{ fontSize: 11 }}>
                  {t.token_count} tokens · {t.span_count || 0} spans
                </p>
              </div>
              <span style={{ color: 'var(--text2)', fontSize: 11 }}>{t.start_time}</span>
            </div>

            {expandedTrace === t.id && false && (
              <div style={{ marginTop: 8, borderTop: '1px solid var(--border)', paddingTop: 8 }}>
              </div>
            )}
          </div>
        ))
      )}
    </div>
  )
}
