import { useState } from 'react'
import * as api from '../api'
interface MemorySearchResult {
  id: string;
  content: string;
  agent_id: string;
  similarity: number;
  timestamp: string;
}

interface Props {
  addToast: (msg: string, type?: 'error' | 'success') => void
}

export function MemoryPanel({ addToast }: Props) {
  const [query, setQuery] = useState('')
  const [results, setResults] = useState<MemorySearchResult[]>([])
  const [searched, setSearched] = useState(false)
  const [searching, setSearching] = useState(false)

  const doSearch = async () => {
    if (!query.trim()) return
    setSearching(true)
    setSearched(true)
    try {
      setResults(await api.searchMemory(query.trim()) as MemorySearchResult[])
    } catch (e: any) { addToast(e.message) }
    finally { setSearching(false) }
  }

  return (
    <div>
      <h3 style={{ fontSize: 13, fontWeight: 600, marginBottom: 8, color: 'var(--accent)' }}>
        Memory Search
      </h3>

      <div style={{ display: 'flex', gap: 4, marginBottom: 12 }}>
        <input value={query} onChange={e => setQuery(e.target.value)}
          onKeyDown={e => e.key === 'Enter' && doSearch()}
          placeholder="Search memories..."
          style={{ flex: 1, padding: '8px 10px', borderRadius: 'var(--radius)',
            border: '1px solid var(--border)', background: 'var(--bg)', color: 'var(--text)', fontSize: 13 }} />
        <button className="btn btn-primary" onClick={doSearch} disabled={!query.trim() || searching}>
          {searching ? '...' : 'Search'}
        </button>
      </div>

      {searched && results.length === 0 && (
        <div className="empty-state"><p>No memories found.</p></div>
      )}

      {results.map(r => (
        <div key={r.id} className="card">
          <p>{r.content}</p>
          <small style={{ display: 'block', marginTop: 4 }}>
            Agent: {r.agent_id} · Similarity: {(r.similarity * 100).toFixed(0)}%
          </small>
        </div>
      ))}

      <h3 style={{ fontSize: 13, fontWeight: 600, marginTop: 16, marginBottom: 8, color: 'var(--accent)' }}>
        Dream Config
      </h3>
      <div className="card" style={{ cursor: 'default' }}>
        <div className="flex-between" style={{ marginBottom: 8 }}>
          <span style={{ fontSize: 13 }}>Dream</span>
          <label style={{ cursor: 'pointer', display: 'flex', alignItems: 'center', gap: 4, fontSize: 12 }}>
            <input type="checkbox" defaultChecked={false} /> Enabled
          </label>
        </div>
        <div className="form-row">
          <div className="form-group">
            <label>Interval (minutes)</label>
            <input type="number" defaultValue={15} />
          </div>
          <div className="form-group">
            <label>Max Memories</label>
            <input type="number" defaultValue={100} />
          </div>
        </div>
        <div className="form-group">
          <label>Consolidation Strategy</label>
          <select defaultValue="summary">
            <option value="summary">Summary</option>
            <option value="cluster">Cluster</option>
            <option value="none">None</option>
          </select>
        </div>
      </div>

      <h3 style={{ fontSize: 13, fontWeight: 600, marginTop: 16, marginBottom: 8, color: 'var(--accent)' }}>
        Retention Settings
      </h3>
      <div className="card" style={{ cursor: 'default' }}>
        <div className="form-row">
          <div className="form-group">
            <label>Max Memories</label>
            <input type="number" defaultValue={1000} />
          </div>
          <div className="form-group">
            <label>TTL (days)</label>
            <input type="number" defaultValue={90} />
          </div>
        </div>
        <div className="form-group">
          <label>Importance Threshold</label>
          <input type="number" step="0.1" min="0" max="1" defaultValue={0.5} />
        </div>
      </div>
    </div>
  )
}
