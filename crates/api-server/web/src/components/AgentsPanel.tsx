import { useState } from 'react'
import * as api from '../api'
import type { AgentSummary, AgentDefinition, ProviderConfig, ToolBinding, ScrollConfig } from '../types'

interface Props {
  agents: AgentSummary[]
  providers: ProviderConfig[]
  selectedAgent: AgentSummary | null
  onSelect: (agent: AgentSummary) => void
  onRefresh: () => void
  addToast: (msg: string, type?: 'error' | 'success') => void
}

const AVAILABLE_TOOLS = [
  { name: 'calculator', desc: 'Arithmetic' },
  { name: 'time', desc: 'Current time' },
  { name: 'shell', desc: 'Shell commands' },
]

const SCROLL_OPTIONS: { value: string; label: string }[] = [
  { value: JSON.stringify({ type: 'truncate', max_messages: 50 }), label: 'Truncate (50)' },
  { value: JSON.stringify({ type: 'truncate', max_messages: 100 }), label: 'Truncate (100)' },
  { value: JSON.stringify({ type: 'sliding_window', window_size: 20 }), label: 'Sliding Window (20)' },
  { value: JSON.stringify({ type: 'no_op' }), label: 'Keep All' },
]

export function AgentsPanel({ agents, providers, selectedAgent, onSelect, onRefresh, addToast }: Props) {
  const [showForm, setShowForm] = useState(false)
  const [editId, setEditId] = useState<string | null>(null)

  // Form fields
  const [name, setName] = useState('')
  const [description, setDescription] = useState('')
  const [providerId, setProviderId] = useState('')
  const [systemPrompt, setSystemPrompt] = useState('')
  const [temperature, setTemperature] = useState('')
  const [maxTokens, setMaxTokens] = useState('')
  const [selectedTools, setSelectedTools] = useState<Set<string>>(new Set())
  const [scrollStr, setScrollStr] = useState(SCROLL_OPTIONS[0].value)

  const openCreate = () => {
    setEditId(null)
    setName('')
    setDescription('')
    setProviderId(providers[0]?.id || '')
    setSystemPrompt('You are a helpful assistant.')
    setTemperature('')
    setMaxTokens('')
    setSelectedTools(new Set(['calculator', 'time']))
    setScrollStr(SCROLL_OPTIONS[0].value)
    setShowForm(true)
  }

  const openEdit = async (agentId: string) => {
    try {
      const a: AgentDefinition = await api.getAgent(agentId)
      setEditId(a.id)
      setName(a.name)
      setDescription(a.description || '')
      setProviderId(a.provider_id)
      setSystemPrompt(a.system_prompt)
      setTemperature(a.temperature != null ? String(a.temperature) : '')
      setMaxTokens(a.max_tokens != null ? String(a.max_tokens) : '')
      setSelectedTools(new Set(
        a.tools.filter((t): t is ToolBinding & { type: 'builtin' } => t.type === 'builtin').map(t => t.name)
      ))
      setScrollStr(JSON.stringify(a.scroll_strategy))
      setShowForm(true)
    } catch (e: any) {
      addToast(e.message)
    }
  }

  const toggleTool = (tool: string) => {
    setSelectedTools(prev => {
      const next = new Set(prev)
      if (next.has(tool)) next.delete(tool)
      else next.add(tool)
      return next
    })
  }

  const save = async () => {
    const tools: ToolBinding[] = Array.from(selectedTools).map(name => ({
      type: 'builtin' as const,
      name,
      enabled: true,
    }))
    let scrollStrategy: ScrollConfig
    try { scrollStrategy = JSON.parse(scrollStr) }
    catch { scrollStrategy = { type: 'truncate', max_messages: 50 } }

    const body: Partial<AgentDefinition> = {
      name,
      description: description || null,
      provider_id: providerId,
      system_prompt: systemPrompt,
      temperature: temperature ? parseFloat(temperature) : null,
      max_tokens: maxTokens ? parseInt(maxTokens) : null,
      tools,
      scroll_strategy: scrollStrategy,
    }

    try {
      if (editId) {
        await api.updateAgent(editId, body)
      } else {
        await api.createAgent(body)
      }
      setShowForm(false)
      onRefresh()
      addToast('Agent saved', 'success')
    } catch (e: any) {
      addToast(e.message)
    }
  }

  const remove = async (id: string) => {
    if (!confirm('Delete this agent?')) return
    try {
      await api.deleteAgent(id)
      onRefresh()
      addToast('Agent deleted', 'success')
    } catch (e: any) {
      addToast(e.message)
    }
  }

  return (
    <>
      <button className="btn btn-primary btn-sm" onClick={openCreate} style={{ width: '100%', marginBottom: 8 }}>
        + New Agent
      </button>

      {agents.length === 0 ? (
        <div className="empty-state"><p>No agents yet.<br />Create one to get started.</p></div>
      ) : (
        agents.map(a => (
          <div key={a.id}
            className={`card${selectedAgent?.id === a.id ? ' active' : ''}`}
            onClick={() => onSelect(a)}
            onDoubleClick={() => openEdit(a.id)}
          >
            <div className="flex-between">
              <h3>{a.name}</h3>
              <div style={{ display: 'flex', gap: 4 }}>
                <button className="btn btn-outline btn-sm"
                  onClick={e => { e.stopPropagation(); openEdit(a.id) }}
                  title="Edit">✎</button>
                <button className="btn btn-danger btn-sm"
                  onClick={e => { e.stopPropagation(); remove(a.id) }}>✕</button>
              </div>
            </div>
            <p>{a.system_prompt}</p>
            <small>{providers.find(p => p.id === a.provider_id)?.label || a.provider_id} · {a.tool_count} tools</small>
          </div>
        ))
      )}

      {/* Agent form modal */}
      <div className={`modal-overlay${showForm ? ' open' : ''}`}>
        <div className="modal">
          <h2>{editId ? 'Edit Agent' : 'New Agent'}</h2>

          <div className="form-group">
            <label>Name</label>
            <input value={name} onChange={e => setName(e.target.value)} placeholder="My Assistant" />
          </div>

          <div className="form-group">
            <label>Description</label>
            <input value={description} onChange={e => setDescription(e.target.value)} placeholder="Optional description" />
          </div>

          <div className="form-group">
            <label>Provider</label>
            {providers.length === 0 ? (
              <div style={{ color: 'var(--red)', fontSize: 12, padding: '8px 0' }}>
                No providers configured. Create one in the Providers tab first.
              </div>
            ) : (
              <select value={providerId} onChange={e => setProviderId(e.target.value)}>
                {providers.map(p => (
                  <option key={p.id} value={p.id}>{p.label} ({p.kind})</option>
                ))}
              </select>
            )}
          </div>

          <div className="form-group">
            <label>System Prompt</label>
            <textarea value={systemPrompt} onChange={e => setSystemPrompt(e.target.value)}
              placeholder="You are a helpful assistant."
              style={{ minHeight: 80 }}
            />
          </div>

          <div className="form-row">
            <div className="form-group">
              <label>Temperature</label>
              <input value={temperature} onChange={e => setTemperature(e.target.value)}
                type="number" step="0.1" placeholder="Default" />
            </div>
            <div className="form-group">
              <label>Max Tokens</label>
              <input value={maxTokens} onChange={e => setMaxTokens(e.target.value)}
                type="number" placeholder="Default" />
            </div>
          </div>

          <div className="form-group">
            <label>Tools</label>
            <div className="tool-chips">
              {AVAILABLE_TOOLS.map(t => (
                <span key={t.name}
                  className={`tool-chip${selectedTools.has(t.name) ? ' selected' : ''}`}
                  onClick={() => toggleTool(t.name)}
                  title={t.desc}
                >{t.name}</span>
              ))}
            </div>
          </div>

          <div className="form-group">
            <label>Scroll Strategy</label>
            <select value={scrollStr} onChange={e => setScrollStr(e.target.value)}>
              {SCROLL_OPTIONS.map((o, i) => (
                <option key={i} value={o.value}>{o.label}</option>
              ))}
            </select>
          </div>

          <div className="form-actions">
            <button className="btn btn-outline" onClick={() => setShowForm(false)}>Cancel</button>
            <button className="btn btn-primary" onClick={save} disabled={providers.length === 0}>Save</button>
          </div>
        </div>
      </div>
    </>
  )
}
