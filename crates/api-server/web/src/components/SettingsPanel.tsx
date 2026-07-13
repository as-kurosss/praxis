import { useState, useEffect } from 'react'
import * as api from '../api'
import type { AppSettings, ScrollConfig } from '../types'

interface Props {
  addToast: (msg: string, type?: 'error' | 'success') => void
}

const SCROLL_OPTIONS: { value: string; label: string }[] = [
  { value: JSON.stringify({ type: 'truncate', max_messages: 50 }), label: 'Truncate (50)' },
  { value: JSON.stringify({ type: 'truncate', max_messages: 100 }), label: 'Truncate (100)' },
  { value: JSON.stringify({ type: 'sliding_window', window_size: 20 }), label: 'Sliding Window (20)' },
  { value: JSON.stringify({ type: 'no_op' }), label: 'Keep All' },
]

export function SettingsPanel({ addToast }: Props) {
  const [settings, setSettings] = useState<AppSettings | null>(null)
  const [loading, setLoading] = useState(true)
  const [dirty, setDirty] = useState(false)

  const load = async () => {
    setLoading(true)
    try { setSettings(await api.getSettings()) }
    catch (e: any) { addToast(e.message) }
    finally { setLoading(false) }
  }

  useEffect(() => { load() }, [])

  const update = async (patch: Partial<AppSettings>) => {
    if (!settings) return
    try {
      const result = await api.updateSettings(patch)
      setSettings(result)
      setDirty(false)
      addToast('Settings saved', 'success')
    } catch (e: any) { addToast(e.message) }
  }

  if (loading) return <div className="empty-state"><p>Loading settings...</p></div>
  if (!settings) return <div className="empty-state"><p>Failed to load settings.</p></div>

  return (
    <div>
      <h3 style={{ fontSize: 13, fontWeight: 600, marginBottom: 12, color: 'var(--accent)' }}>
        Default Settings
      </h3>

      <div className="form-group">
        <label>Default Model</label>
        <input value={settings.default_model} onChange={e => {
          setSettings({ ...settings, default_model: e.target.value })
          setDirty(true)
        }} placeholder="gpt-4o" />
      </div>

      <div className="form-group">
        <label>Default Scroll Strategy</label>
        <select value={JSON.stringify(settings.default_scroll_strategy)} onChange={e => {
          try {
            const scroll: ScrollConfig = JSON.parse(e.target.value)
            setSettings({ ...settings, default_scroll_strategy: scroll })
            setDirty(true)
          } catch { /* ignore */ }
        }}>
          {SCROLL_OPTIONS.map((o, i) => (
            <option key={i} value={o.value}>{o.label}</option>
          ))}
        </select>
      </div>

      <div className="form-row">
        <div className="form-group">
          <label>Default Temperature</label>
          <input type="number" step="0.1" value={settings.default_temperature ?? ''}
            onChange={e => {
              const v = e.target.value ? parseFloat(e.target.value) : null
              setSettings({ ...settings, default_temperature: v })
              setDirty(true)
            }} placeholder="None (provider default)" />
        </div>
        <div className="form-group">
          <label>Default Max Tokens</label>
          <input type="number" value={settings.default_max_tokens ?? ''}
            onChange={e => {
              const v = e.target.value ? parseInt(e.target.value) : null
              setSettings({ ...settings, default_max_tokens: v })
              setDirty(true)
            }} placeholder="None (provider default)" />
        </div>
      </div>

      <div className="form-row">
        <div className="form-group">
          <label>Theme</label>
          <select value={settings.theme} onChange={e => {
            setSettings({ ...settings, theme: e.target.value as 'dark' | 'light' })
            setDirty(true)
          }}>
            <option value="dark">Dark</option>
            <option value="light">Light</option>
          </select>
        </div>
        <div className="form-group">
          <label>Language</label>
          <select value={settings.language} onChange={e => {
            setSettings({ ...settings, language: e.target.value })
            setDirty(true)
          }}>
            <option value="en">English</option>
            <option value="ru">Русский</option>
          </select>
        </div>
      </div>

      {dirty && (
        <button className="btn btn-primary" style={{ width: '100%', marginTop: 8 }}
          onClick={() => update(settings)}>
          Save Settings
        </button>
      )}
    </div>
  )
}
