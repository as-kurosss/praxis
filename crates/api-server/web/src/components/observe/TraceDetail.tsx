import { useEffect, useState } from 'react'
import * as api from '../../api'
import type { TraceSummary, SpanSummary } from '../../types'

interface Props {
  trace: TraceSummary
  onClose: () => void
}

function statusClass(status: string): string {
  switch (status) {
    case 'completed': return 'badge-ok'
    case 'failed': return 'badge-fail'
    case 'active': return 'badge-pending'
    default: return ''
  }
}

function formatTime(iso: string): string {
  return new Date(iso).toLocaleString()
}

function spanDuration(span: SpanSummary): string {
  if (!span.start_time) return '—'
  const start = new Date(span.start_time).getTime()
  const end = span.end_time ? new Date(span.end_time).getTime() : Date.now()
  const ms = end - start
  if (ms < 1000) return `${ms}ms`
  return `${(ms / 1000).toFixed(2)}s`
}

function traceDuration(t: TraceSummary): string {
  if (!t.start_time) return '—'
  const start = new Date(t.start_time).getTime()
  const end = t.end_time ? new Date(t.end_time).getTime() : Date.now()
  const ms = end - start
  if (ms < 1000) return `${ms}ms`
  return `${(ms / 1000).toFixed(2)}s`
}

export function TraceDetail({ trace, onClose }: Props) {
  const [spans, setSpans] = useState<SpanSummary[]>([])
  const [loading, setLoading] = useState(true)

  useEffect(() => {
    let cancelled = false
    setLoading(true)
    api.getTraceSpans(trace.id).then((data: any) => {
      if (!cancelled) {
        setSpans(data)
        setLoading(false)
      }
    }).catch(() => {
      if (!cancelled) setLoading(false)
    })
    return () => { cancelled = true }
  }, [trace.id])

  // Compute time range for waterfall
  const allTimes = [trace.start_time, ...spans.map(s => s.start_time), ...spans.filter(s => s.end_time).map(s => s.end_time!), trace.end_time].filter(Boolean) as string[]
  const minTime = allTimes.length > 0 ? Math.min(...allTimes.map(t => new Date(t).getTime())) : 0
  const maxTime = allTimes.length > 0 ? Math.max(...allTimes.map(t => new Date(t).getTime())) : 1
  const totalSpan = maxTime - minTime || 1

  return (
    <div className="trace-detail-overlay" onClick={onClose}>
      <div className="trace-detail-panel" onClick={e => e.stopPropagation()}>
        <div className="trace-detail-header">
          <h3>Trace Detail</h3>
          <button className="btn btn-ghost btn-sm" onClick={onClose}>✕</button>
        </div>

        <div className="trace-detail-meta">
          <div className="meta-row">
            <span className="meta-label">ID</span>
            <span className="meta-value">{trace.id}</span>
          </div>
          <div className="meta-row">
            <span className="meta-label">Agent</span>
            <span className="meta-value">{trace.agent_id}</span>
          </div>
          <div className="meta-row">
            <span className="meta-label">Status</span>
            <span className={`badge ${statusClass(trace.status)}`}>{trace.status}</span>
          </div>
          <div className="meta-row">
            <span className="meta-label">Duration</span>
            <span className="meta-value">{traceDuration(trace)}</span>
          </div>
          <div className="meta-row">
            <span className="meta-label">Tokens</span>
            <span className="meta-value">{trace.token_count ?? '—'}</span>
          </div>
          <div className="meta-row">
            <span className="meta-label">Started</span>
            <span className="meta-value">{formatTime(trace.start_time)}</span>
          </div>
          {trace.error && (
            <div className="meta-row">
              <span className="meta-label">Error</span>
              <span className="meta-value meta-error">{trace.error}</span>
            </div>
          )}
        </div>

        <h4 style={{ margin: '12px 0 8px', fontSize: 14 }}>Spans ({spans.length})</h4>

        {loading ? (
          <div className="empty-state">Loading spans…</div>
        ) : spans.length === 0 ? (
          <div className="empty-state">No spans for this trace</div>
        ) : (
          <div className="waterfall">
            <div className="waterfall-header">
              <div className="waterfall-name-header">Name</div>
              <div className="waterfall-bar-header">Timeline</div>
              <div className="waterfall-dur-header">Duration</div>
            </div>
            {spans.map(s => {
              const sStart = new Date(s.start_time).getTime()
              const sEnd = s.end_time ? new Date(s.end_time).getTime() : Date.now()
              const left = ((sStart - minTime) / totalSpan) * 100
              const width = Math.max(((sEnd - sStart) / totalSpan) * 100, 2)
              return (
                <div key={s.id} className="waterfall-row">
                  <div className="waterfall-name" title={s.name}>
                    <span className="span-name-dot" /> {s.name}
                  </div>
                  <div className="waterfall-bar">
                    <div
                      className="waterfall-bar-fill"
                      style={{ left: `${left}%`, width: `${width}%` }}
                    />
                  </div>
                  <div className="waterfall-dur">{spanDuration(s)}</div>
                </div>
              )
            })}
          </div>
        )}

        {/* Selected span details */}
        {spans.filter(s => s.metadata && s.metadata !== null).length > 0 && (
          <>
            <h4 style={{ margin: '16px 0 8px', fontSize: 14 }}>Details</h4>
            {spans.filter(s => s.metadata && (s.metadata as any) !== null).map(s => {
              const meta = s.metadata as Record<string, unknown>
              return (
                <div key={s.id} className="span-detail-card">
                  <div className="span-detail-title">{s.name}</div>
                  {s.token_count != null && (
                    <div className="span-detail-info">Tokens: {s.token_count}</div>
                  )}
                  {meta && Object.keys(meta).length > 0 && (
                    <pre className="span-detail-meta-pre">{JSON.stringify(meta, null, 2)}</pre>
                  )}
                </div>
              )
            })}
          </>
        )}
      </div>
    </div>
  )
}
