import type { MetricPoint } from '../../types'

interface Props {
  metrics: MetricPoint[]
}

export function MetricsCharts({ metrics }: Props) {
  const tokenMetrics = metrics.filter(m => m.name === 'token_usage' || m.name === 'token_count').slice(-30)
  const latencyMetrics = metrics.filter(m => m.name === 'latency_ms').slice(-30)

  return (
    <div className="metrics-charts">
      <div className="chart-card">
        <h4>Token Usage (recent)</h4>
        {tokenMetrics.length === 0 ? (
          <div className="empty-state" style={{ padding: 20 }}>No token data yet</div>
        ) : (
          <SimpleLineChart data={tokenMetrics} valueKey="value" />
        )}
      </div>
      <div className="chart-card">
        <h4>Latency (recent)</h4>
        {latencyMetrics.length === 0 ? (
          <div className="empty-state" style={{ padding: 20 }}>No latency data yet</div>
        ) : (
          <SimpleLineChart data={latencyMetrics} valueKey="value" />
        )}
      </div>
    </div>
  )
}

// ── Simple SVG line chart ──────────────────────────────────

function SimpleLineChart({ data, valueKey }: { data: MetricPoint[]; valueKey: 'value' }) {
  const w = 280, h = 100
  const pad = { top: 10, right: 10, bottom: 20, left: 40 }
  const innerW = w - pad.left - pad.right
  const innerH = h - pad.top - pad.bottom

  const values = data.map(d => d[valueKey])
  const max = Math.max(...values, 1)
  const min = Math.min(...values, 0)
  const range = max - min || 1

  const points = data.map((d, i) => {
    const x = pad.left + (i / Math.max(data.length - 1, 1)) * innerW
    const y = pad.top + innerH - ((d[valueKey] - min) / range) * innerH
    return `${x},${y}`
  })

  const polyline = points.join(' ')

  return (
    <svg width={w} height={h} style={{ display: 'block' }}>
      {/* Y axis labels */}
      <text x={pad.left - 6} y={pad.top + 4} textAnchor="end" fontSize={9} fill="var(--text2)">
        {max.toFixed(0)}
      </text>
      <text x={pad.left - 6} y={pad.top + innerH + 4} textAnchor="end" fontSize={9} fill="var(--text2)">
        {min.toFixed(0)}
      </text>
      {/* Grid lines */}
      <line x1={pad.left} y1={pad.top} x2={pad.left + innerW} y2={pad.top}
        stroke="var(--border)" strokeWidth={0.5} />
      <line x1={pad.left} y1={pad.top + innerH} x2={pad.left + innerW} y2={pad.top + innerH}
        stroke="var(--border)" strokeWidth={0.5} />
      {/* Line */}
      <polyline points={polyline} fill="none" stroke="var(--accent)" strokeWidth={2}
        strokeLinejoin="round" strokeLinecap="round" />
      {/* Dots */}
      {data.map((d, i) => {
        const parts = points[i].split(',')
        return <circle key={i} cx={parts[0]} cy={parts[1]} r={2} fill="var(--accent)" />
      })}
    </svg>
  )
}
