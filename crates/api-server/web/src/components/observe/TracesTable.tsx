import type { TraceSummary } from '../../types'

interface Props {
  traces: TraceSummary[]
  onSelect: (trace: TraceSummary) => void
  selectedId: string | null
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
  const d = new Date(iso)
  return d.toLocaleString()
}

function duration(t: TraceSummary): string {
  if (!t.start_time) return '—'
  const start = new Date(t.start_time).getTime()
  const end = t.end_time ? new Date(t.end_time).getTime() : Date.now()
  const ms = end - start
  if (ms < 1000) return `${ms}ms`
  return `${(ms / 1000).toFixed(1)}s`
}

export function TracesTable({ traces, onSelect, selectedId }: Props) {
  return (
    <div className="traces-table-container">
      <table className="traces-table">
        <thead>
          <tr>
            <th>ID</th>
            <th>Type</th>
            <th>Status</th>
            <th>Duration</th>
            <th>Tokens</th>
            <th>Timestamp</th>
          </tr>
        </thead>
        <tbody>
          {traces.length === 0 ? (
            <tr>
              <td colSpan={6} className="empty-cell">No traces recorded yet</td>
            </tr>
          ) : (
            traces.map(t => (
              <tr
                key={t.id}
                className={`trace-row${selectedId === t.id ? ' selected' : ''}`}
                onClick={() => onSelect(t)}
              >
                <td className="trace-id" title={t.id}>{t.id.slice(0, 8)}…</td>
                <td>{t.agent_id}</td>
                <td>
                  <span className={`badge ${statusClass(t.status)}`}>
                    {t.status}
                  </span>
                </td>
                <td>{duration(t)}</td>
                <td>{t.token_count ?? '—'}</td>
                <td>{formatTime(t.start_time)}</td>
              </tr>
            ))
          )}
        </tbody>
      </table>
    </div>
  )
}
