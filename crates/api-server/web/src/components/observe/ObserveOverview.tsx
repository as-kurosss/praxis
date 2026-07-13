import * as api from '../../api'
import type { DashboardStats } from '../../types'
import { useEffect, useState } from 'react'

interface Props {
  stats: DashboardStats | null
  onRefresh: () => void
}

export function ObserveOverview({ stats, onRefresh }: Props) {
  return (
    <div className="observe-overview">
      <div className="stat-card">
        <div className="stat-value">{stats?.total_traces ?? '—'}</div>
        <div className="stat-label">Total Traces (24h)</div>
      </div>
      <div className="stat-card">
        <div className="stat-value">
          {stats?.avg_latency_ms != null
            ? `${(stats.avg_latency_ms).toFixed(0)}ms`
            : '—'}
        </div>
        <div className="stat-label">Avg Latency</div>
      </div>
      <div className="stat-card">
        <div className="stat-value">
          {stats != null
            ? stats.total_traces > 0
              ? `${((stats.failed_traces / stats.total_traces) * 100).toFixed(1)}%`
              : '0%'
            : '—'}
        </div>
        <div className="stat-label">Error Rate</div>
      </div>
      <div className="stat-card">
        <div className="stat-value">
          {stats?.total_tokens != null
            ? stats.total_tokens.toLocaleString()
            : '—'}
        </div>
        <div className="stat-label">Token Usage</div>
      </div>
    </div>
  )
}
