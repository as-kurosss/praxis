import { useState } from 'react'
import * as api from '../api'
import type { ProviderConfig, ProviderKind } from '../types'

interface Props {
  providers: ProviderConfig[]
  onRefresh: () => void
  addToast: (msg: string, type?: 'error' | 'success') => void
}

const PROVIDER_KINDS: { value: ProviderKind; label: string; defaultUrl: string }[] = [
  { value: 'openai', label: 'OpenAI', defaultUrl: 'https://api.openai.com/v1' },
  { value: 'anthropic', label: 'Anthropic', defaultUrl: 'https://api.anthropic.com/v1' },
  { value: 'gemini', label: 'Gemini', defaultUrl: 'https://generativelanguage.googleapis.com/v1beta' },
  { value: 'ollama', label: 'Ollama', defaultUrl: 'http://localhost:11434/v1' },
]

const DEFAULT_MODELS: Record<ProviderKind, string> = {
  openai: 'gpt-4o',
  anthropic: 'claude-3-5-sonnet-20241022',
  gemini: 'gemini-2.0-flash',
  ollama: 'llama3.2',
}

export function ProvidersPanel({ providers, onRefresh, addToast }: Props) {
  const [editing, setEditing] = useState<ProviderConfig | null>(null)
  const [showForm, setShowForm] = useState(false)

  const [kind, setKind] = useState<ProviderKind>('openai')
  const [label, setLabel] = useState('')
  const [model, setModel] = useState('')
  const [apiKey, setApiKey] = useState('')
  const [apiUrl, setApiUrl] = useState('')

  const openCreate = () => {
    setEditing(null)
    setKind('openai')
    setLabel('')
    setModel('gpt-4o')
    setApiKey('')
    setApiUrl('https://api.openai.com/v1')
    setShowForm(true)
  }

  const openEdit = (p: ProviderConfig) => {
    setEditing(p)
    setKind(p.kind)
    setLabel(p.label)
    setModel(p.model)
    setApiKey(p.api_key)
    setApiUrl(p.api_url || '')
    setShowForm(true)
  }

  const handleKindChange = (k: ProviderKind) => {
    setKind(k)
    setModel(DEFAULT_MODELS[k])
    setApiUrl(PROVIDER_KINDS.find(x => x.value === k)?.defaultUrl || '')
  }

  const save = async () => {
    const body = { kind, label, model, api_key: apiKey, api_url: apiUrl || null }
    try {
      if (editing) {
        await api.updateProvider(editing.id, body)
      } else {
        await api.createProvider(body)
      }
      setShowForm(false)
      onRefresh()
      addToast('Provider saved', 'success')
    } catch (e: any) {
      addToast(e.message)
    }
  }

  const remove = async (id: string) => {
    if (!confirm('Delete this provider?')) return
    try {
      await api.deleteProvider(id)
      onRefresh()
      addToast('Provider deleted', 'success')
    } catch (e: any) {
      addToast(e.message)
    }
  }

  return (
    <>
      <button className="btn btn-primary btn-sm" onClick={openCreate} style={{width:'100%',marginBottom:8}}>
        + New Provider
      </button>
      {providers.length === 0 ? (
        <div className="empty-state"><p>No providers configured yet.</p></div>
      ) : (
        providers.map(p => (
          <div className="card" key={p.id} onClick={() => openEdit(p)}>
            <div className="flex-between">
              <h3>{p.label} <span className={`badge badge-${p.kind}`}>{p.kind}</span></h3>
              <button className="btn btn-danger btn-sm" onClick={e => { e.stopPropagation(); remove(p.id) }}>✕</button>
            </div>
            <p>{p.model}</p>
            <small>{p.id}</small>
          </div>
        ))
      )}

      {/* Provider form modal */}
      <div className={`modal-overlay${showForm ? ' open' : ''}`}>
        <div className="modal">
          <h2>{editing ? 'Edit Provider' : 'New Provider'}</h2>
          <div className="form-group">
            <label>Label</label>
            <input value={label} onChange={e => setLabel(e.target.value)} placeholder="My OpenAI Key" />
          </div>
          <div className="form-row">
            <div className="form-group">
              <label>Provider</label>
              <select value={kind} onChange={e => handleKindChange(e.target.value as ProviderKind)}>
                {PROVIDER_KINDS.map(k => <option key={k.value} value={k.value}>{k.label}</option>)}
              </select>
            </div>
            <div className="form-group">
              <label>Model</label>
              <input value={model} onChange={e => setModel(e.target.value)} placeholder="gpt-4o" />
            </div>
          </div>
          <div className="form-group">
            <label>API Key {kind === 'ollama' ? <span style={{color:'var(--text2)',fontWeight:400}}>(optional for local)</span> : ''}</label>
            <input value={apiKey} onChange={e => setApiKey(e.target.value)} type="password" placeholder={kind === 'ollama' ? 'Leave empty for local' : 'sk-...'} />
          </div>
          <div className="form-group">
            <label>API URL</label>
            <input value={apiUrl} onChange={e => setApiUrl(e.target.value)} placeholder={PROVIDER_KINDS.find(k => k.value === kind)?.defaultUrl || 'https://...'} />
          </div>
          <div className="form-actions">
            <button className="btn btn-outline" onClick={() => setShowForm(false)}>Cancel</button>
            <button className="btn btn-primary" onClick={save}>Save</button>
          </div>
        </div>
      </div>
    </>
  )
}
