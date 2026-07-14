import { useState, useEffect } from 'react'
import type { Config, McpServerConfig } from '../types'
import * as api from '../api'

interface Props {
  config: Config | null
  viewMode: 'normal' | 'wide' | 'simple'
  onViewModeChange: (mode: 'normal' | 'wide' | 'simple') => void
  onClose: () => void
  addToast: (msg: string, type?: 'error' | 'success' | 'info') => void
}

export function SettingsPanel({ config, viewMode, onViewModeChange, onClose, addToast }: Props) {
  const [mcpServers, setMcpServers] = useState<McpServerConfig[]>([])
  const [mcpName, setMcpName] = useState('')
  const [mcpCommand, setMcpCommand] = useState('')
  const [mcpArgs, setMcpArgs] = useState('')

  const loadMcp = async () => {
    try { setMcpServers(await api.listMcpServers()) }
    catch { /* ignore */ }
  }

  useEffect(() => { loadMcp() }, [])

  const addMcpServer = async () => {
    if (!mcpName.trim() || !mcpCommand.trim()) return
    try {
      await api.createMcpServer({ name: mcpName.trim(), command: mcpCommand.trim(), args: mcpArgs.split(' ').filter(Boolean) })
      addToast('MCP server added', 'success')
      setMcpName(''); setMcpCommand(''); setMcpArgs('')
      loadMcp()
    } catch (e: any) { addToast(e.message) }
  }

  const deleteMcpServer = async (name: string) => {
    try {
      await api.deleteMcpServer(name)
      addToast('MCP server removed', 'success')
      loadMcp()
    } catch (e: any) { addToast(e.message) }
  }

  return (
    <div className="modal-overlay open" onClick={onClose}>
      <div className="modal settings-modal" onClick={e => e.stopPropagation()}>
        <div className="flex-between">
          <h2>Settings</h2>
          <button className="btn btn-ghost btn-sm" onClick={onClose}>✕</button>
        </div>

        {/* ── MCP Servers ── */}
        <h3 style={{ fontSize: 13, fontWeight: 600, marginTop: 8, marginBottom: 8, color: 'var(--accent)' }}>
          MCP Servers
        </h3>

        {mcpServers.length === 0 ? (
          <div className="empty-state" style={{ fontSize: 12, marginBottom: 8 }}>
            <p>No MCP servers configured.</p>
          </div>
        ) : (
          mcpServers.map(s => (
            <div key={s.name} className="card" style={{ padding: '8px 10px', marginBottom: 4 }}>
              <div className="flex-between">
                <div>
                  <strong style={{ fontSize: 13 }}>{s.name}</strong>
                  <code style={{ display: 'block', fontSize: 10, color: 'var(--text2)', marginTop: 2 }}>
                    {s.command} {s.args.join(' ')}
                  </code>
                </div>
                <button className="btn btn-ghost btn-sm" onClick={() => deleteMcpServer(s.name)}
                  style={{ color: 'var(--red)' }}>✕</button>
              </div>
            </div>
          ))
        )}

        <div style={{ display: 'flex', flexDirection: 'column', gap: 4, marginBottom: 12 }}>
          <input value={mcpName} onChange={e => setMcpName(e.target.value)}
            placeholder="Server name (e.g. brave-search)"
            style={{ padding: '6px 8px', fontSize: 12, borderRadius: 'var(--radius)',
              border: '1px solid var(--border)', background: 'var(--bg)', color: 'var(--text)' }} />
          <input value={mcpCommand} onChange={e => setMcpCommand(e.target.value)}
            placeholder="Command (e.g. npx)"
            style={{ padding: '6px 8px', fontSize: 12, borderRadius: 'var(--radius)',
              border: '1px solid var(--border)', background: 'var(--bg)', color: 'var(--text)' }} />
          <input value={mcpArgs} onChange={e => setMcpArgs(e.target.value)}
            placeholder="Args (space-separated, e.g. -y @anthropic/mcp-serve)"
            style={{ padding: '6px 8px', fontSize: 12, borderRadius: 'var(--radius)',
              border: '1px solid var(--border)', background: 'var(--bg)', color: 'var(--text)' }} />
          <button className="btn btn-primary btn-sm" onClick={addMcpServer}
            disabled={!mcpName.trim() || !mcpCommand.trim()}>Add Server</button>
        </div>

        <hr className="settings-divider" />

        {/* Request Timeout */}
        <div className="form-group">
          <label>Request Timeout (seconds)</label>
          <div className="setting-value">
            {config?.request_timeout_seconds ?? 30}s
          </div>
          <div className="setting-hint">
            Maximum time the server waits for an LLM response before timing out.
          </div>
        </div>

        {/* Owner */}
        {config?.owner_id && (
          <div className="form-group">
            <label>Session Owner</label>
            <div className="setting-value">{config.owner_id}</div>
          </div>
        )}

        <hr className="settings-divider" />

        {/* View Mode */}
        <div className="form-group">
          <label>View Mode</label>
          <div className="view-mode-options">
            <button
              className={`btn ${viewMode === 'normal' ? 'btn-primary' : 'btn-outline'} btn-sm`}
              onClick={() => onViewModeChange('normal')}
            >
              Normal
            </button>
            <button
              className={`btn ${viewMode === 'wide' ? 'btn-primary' : 'btn-outline'} btn-sm`}
              onClick={() => onViewModeChange('wide')}
            >
              Wide
            </button>
            <button
              className={`btn ${viewMode === 'simple' ? 'btn-primary' : 'btn-outline'} btn-sm`}
              onClick={() => onViewModeChange('simple')}
            >
              Simple
            </button>
          </div>
          <div className="setting-hint" style={{ marginTop: 4 }}>
            {viewMode === 'normal' && 'Sidebar visible, standard layout.'}
            {viewMode === 'wide' && 'Sidebar hidden, full-width chat area.'}
            {viewMode === 'simple' && 'Minimal UI — sidebar hidden, flat navigation, compact.'}
          </div>
        </div>
      </div>
    </div>
  )
}
