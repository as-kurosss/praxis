import { useState } from 'react'
import type { ToolBinding } from '../types'
import { BUILTIN_TOOLS } from '../types'

interface Props {
  tools: ToolBinding[]
  onToolsChange: (tools: ToolBinding[]) => void
}

export function ToolsPanel({ tools, onToolsChange }: Props) {
  const enabledTools = tools.filter(t =>
    (t.type === 'builtin' && t.enabled) || (t.type === 'custom' && t.enabled)
  )
  const availableTools = BUILTIN_TOOLS.map(bt => {
    const existing = tools.find(t => t.type === 'builtin' && t.name === bt.name)
    return { ...bt, enabled: existing ? existing.enabled : false }
  })

  const toggleTool = (name: string, enabled: boolean) => {
    const existing = tools.find(t => t.type === 'builtin' && t.name === name)
    if (existing) {
      onToolsChange(tools.map(t =>
        t.type === 'builtin' && t.name === name ? { ...t, enabled } : t
      ))
    } else {
      onToolsChange([...tools, { type: 'builtin' as const, name, enabled }])
    }
  }

  return (
    <div>
      <h3 style={{ fontSize: 13, fontWeight: 600, marginBottom: 8, color: 'var(--accent)' }}>
        Enabled Tools ({enabledTools.length})
      </h3>
      {enabledTools.length === 0 ? (
        <div className="empty-state" style={{ padding: 16, fontSize: 12 }}>
          <p>No tools enabled. Toggle tools below to add them.</p>
        </div>
      ) : (
        enabledTools.map(t => (
          <div key={t.type === 'builtin' ? t.name : t.name} className="card" style={{ padding: '8px 10px' }}>
            <div className="flex-between">
              <div>
                <strong style={{ fontSize: 13 }}>{t.name}</strong>
                {t.type === 'custom' && <span className="badge badge-ollama" style={{ marginLeft: 6 }}>custom</span>}
              </div>
              <button className="btn btn-ghost btn-sm" onClick={() => toggleTool(t.name, false)}
                title="Disable">✕</button>
            </div>
          </div>
        ))
      )}

      <h3 style={{ fontSize: 13, fontWeight: 600, marginTop: 16, marginBottom: 8, color: 'var(--text2)' }}>
        Available Tools
      </h3>
      {availableTools.map(t => (
        <div key={t.name} className="card" style={{ padding: '8px 10px', opacity: t.enabled ? 0.5 : 1 }}
          onClick={() => toggleTool(t.name, true)}>
          <div className="flex-between">
            <div>
              <span style={{ fontSize: 13 }}>{t.name}</span>
              <p style={{ fontSize: 11 }}>{t.description}</p>
            </div>
            {!t.enabled && <button className="btn btn-primary btn-sm">Enable</button>}
          </div>
        </div>
      ))}
    </div>
  )
}
