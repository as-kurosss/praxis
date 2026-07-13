import { useEffect, useState, useCallback } from 'react'
import * as api from '../../api'
import type { TraceSummary, MetricPoint, DashboardStats } from '../../types'
import { ObserveOverview } from './ObserveOverview'
import { TracesTable } from './TracesTable'
import { TraceDetail } from './TraceDetail'
import { MetricsCharts } from './MetricsCharts'

interface Props {
  addToast: (msg: string, type?: 'error' | 'success') => void
}

export function ObservePage({ addToast }: Props) {
  const [traces, setTraces] = useState<TraceSummary[]>([])
  const [metrics, setMetrics] = useState<MetricPoint[]>([])
  const [stats, setStats] = useState<DashboardStats | null>(null)
  const [selectedTrace, setSelectedTrace] = useState<TraceSummary | null>(null)
  const [filter, setFilter] = useState('')

  const loadData = useCallback(async () => {
    try {
      const [t, m, s] = await Promise.all([
        api.listTraces(),
        api.listMetrics(),
        api.getDashboardStats(),
      ])
      setTraces(t)
      setMetrics(m)
      setStats(s)
    } catch (e: any) {
      addToast(e.message)
    }
  }, [addToast])

  useEffect(() => {
    loadData()
    const interval = setInterval(loadData, 10000)
    return () => clearInterval(interval)
  }, [loadData])

  const filteredTraces = filter
    ? traces.filter(t =>
        t.id.toLowerCase().includes(filter.toLowerCase()) ||
        t.agent_id.toLowerCase().includes(filter.toLowerCase()) ||
        t.status.toLowerCase().includes(filter.toLowerCase())
      )
    : traces

  return (
    <div className="observe-page">
      <div className="observe-toolbar">
        <h2>Observe Dashboard</h2>
        <button className="btn btn-outline btn-sm" onClick={loadData}>Refresh</button>
      </div>

      <ObserveOverview stats={stats} onRefresh={loadData} />

      <div className="observe-section">
        <div className="observe-section-header">
          <h3>Traces</h3>
          <input
            className="observe-search"
            type="text"
            placeholder="Search traces…"
            value={filter}
            onChange={e => setFilter(e.target.value)}
          />
        </div>
        <TracesTable
          traces={filteredTraces}
          onSelect={setSelectedTrace}
          selectedId={selectedTrace?.id ?? null}
        />
      </div>

      <div className="observe-section">
        <div className="observe-section-header">
          <h3>Metrics</h3>
        </div>
        <MetricsCharts metrics={metrics} />
      </div>

      {selectedTrace && (
        <TraceDetail
          trace={selectedTrace}
          onClose={() => setSelectedTrace(null)}
        />
      )}
    </div>
  )
}
