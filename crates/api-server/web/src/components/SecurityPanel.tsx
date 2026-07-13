import { useState, useEffect } from 'react'
import * as api from '../api'
import type { SecurityPolicy } from '../types'

interface Props {
  addToast: (msg: string, type?: 'error' | 'success') => void
}

export function SecurityPanel({ addToast }: Props) {
  const [policies, setPolicies] = useState<SecurityPolicy[]>([])
  const [loading, setLoading] = useState(true)
  const [expanded, setExpanded] = useState<string | null>(null)

  const load = async () => {
    setLoading(true)
    try { setPolicies(await api.listSecurityPolicies()) }
    catch (e: any) { addToast(e.message) }
    finally { setLoading(false) }
  }

  useEffect(() => { load() }, [])

  const actionColor = (action: string) => {
    switch (action) {
      case 'allow': return 'var(--accent)'
      case 'deny': return 'var(--red)'
      case 'ask': return '#d97706'
      default: return 'var(--text2)'
    }
  }

  if (loading) return <div className="empty-state"><p>Loading policies...</p></div>

  return (
    <div>
      <h3 style={{ fontSize: 13, fontWeight: 600, marginBottom: 12, color: 'var(--accent)' }}>
        Security Policies
      </h3>

      {policies.length === 0 ? (
        <div className="empty-state"><p>No policies configured.</p></div>
      ) : (
        policies.map(p => (
          <div key={p.id} className="card">
            <div className="flex-between" onClick={() => setExpanded(expanded === p.id ? null : p.id)}
              style={{ cursor: 'pointer' }}>
              <div>
                <h3>{p.name}</h3>
                <p>{p.description}</p>
              </div>
              <span style={{
                padding: '2px 8px', borderRadius: 'var(--radius-sm)',
                fontSize: 11, fontWeight: 600,
                background: actionColor(p.action), color: '#fff',
              }}>{p.action.toUpperCase()}</span>
            </div>

            {expanded === p.id && p.rules.length > 0 && (
              <div style={{ marginTop: 8, borderTop: '1px solid var(--border)', paddingTop: 8 }}>
                <small style={{ color: 'var(--text2)', fontWeight: 600 }}>Rules</small>
                {p.rules.map(r => (
                  <div key={r.id} className="flex-between" style={{ padding: '6px 0', fontSize: 12 }}>
                    <div>
                      <span>{r.name}</span>
                      <code style={{ display: 'block', color: 'var(--text2)', fontSize: 10, marginTop: 2 }}>{r.pattern}</code>
                    </div>
                    <span style={{
                      padding: '1px 6px', borderRadius: 'var(--radius-sm)',
                      fontSize: 10, fontWeight: 600,
                      background: actionColor(r.action), color: '#fff',
                    }}>{r.action}</span>
                  </div>
                ))}
              </div>
            )}
          </div>
        ))
      )}

      <h3 style={{ fontSize: 13, fontWeight: 600, marginTop: 16, marginBottom: 8, color: 'var(--accent)' }}>
        Sandbox Configuration
      </h3>
      <div className="card" style={{ cursor: 'default' }}>
        <div className="flex-between" style={{ marginBottom: 6 }}>
          <span style={{ fontSize: 13 }}>Docker Sandbox</span>
          <label style={{ cursor: 'pointer', display: 'flex', alignItems: 'center', gap: 4, fontSize: 12 }}>
            <input type="checkbox" defaultChecked={false} /> Enabled
          </label>
        </div>
        <div className="form-group">
          <label>Docker Image</label>
          <input defaultValue="ubuntu:22.04" placeholder="ubuntu:22.04" />
        </div>
        <div className="form-group" style={{ marginTop: 6 }}>
          <label>Timeout (seconds)</label>
          <input type="number" defaultValue={30} />
        </div>
      </div>

      <h3 style={{ fontSize: 13, fontWeight: 600, marginTop: 16, marginBottom: 8, color: 'var(--accent)' }}>
        Shell Evasion Rules
      </h3>
      {[
        { id: 'evade_rm', name: 'Restrict rm -rf', desc: 'Block recursive force delete' },
        { id: 'evade_curl', name: 'Restrict curl/wget', desc: 'Ask before network requests' },
        { id: 'evade_nc', name: 'Restrict netcat', desc: 'Block reverse shells' },
        { id: 'evade_chmod', name: 'Restrict chmod 777', desc: 'Block world-writable permissions' },
      ].map(rule => (
        <div key={rule.id} className="card" style={{ padding: '8px 10px', cursor: 'default' }}>
          <div className="flex-between">
            <div>
              <span style={{ fontSize: 13 }}>{rule.name}</span>
              <p style={{ fontSize: 11 }}>{rule.desc}</p>
            </div>
            <label style={{ cursor: 'pointer', display: 'flex', alignItems: 'center', gap: 4 }}>
              <input type="checkbox" defaultChecked={rule.id === 'evade_rm'} />
            </label>
          </div>
        </div>
      ))}
    </div>
  )
}
