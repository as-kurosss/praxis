import { useState, useEffect } from 'react'
import * as api from '../api'
import type { SkillDefinition } from '../types'

interface Props {
  addToast: (msg: string, type?: 'error' | 'success') => void
}

export function SkillsPanel({ addToast }: Props) {
  const [skills, setSkills] = useState<SkillDefinition[]>([])
  const [loading, setLoading] = useState(true)
  const [importUrl, setImportUrl] = useState('')
  const [showImport, setShowImport] = useState(false)

  const load = async () => {
    setLoading(true)
    try { setSkills(await api.listSkills()) }
    catch (e: any) { addToast(e.message) }
    finally { setLoading(false) }
  }

  useEffect(() => { load() }, [])

  const toggle = async (id: string, enabled: boolean) => {
    try {
      await api.toggleSkill(id, enabled)
      await load()
      addToast('Skill updated', 'success')
    } catch (e: any) { addToast(e.message) }
  }

  const remove = async (id: string) => {
    if (!confirm('Delete this skill?')) return
    try {
      await api.deleteSkill(id)
      await load()
      addToast('Skill deleted', 'success')
    } catch (e: any) { addToast(e.message) }
  }

  const doImport = async () => {
    if (!importUrl.trim()) return
    try {
      await api.importSkill(importUrl.trim())
      setImportUrl('')
      setShowImport(false)
      await load()
      addToast('Skill imported', 'success')
    } catch (e: any) { addToast(e.message) }
  }

  if (loading) return <div className="empty-state"><p>Loading skills...</p></div>

  return (
    <div>
      <div style={{ display: 'flex', gap: 4, marginBottom: 8 }}>
        <button className="btn btn-primary btn-sm" onClick={() => setShowImport(true)} style={{ flex: 1 }}>
          + Import Skill
        </button>
      </div>

      {skills.length === 0 ? (
        <div className="empty-state"><p>No skills imported yet.</p></div>
      ) : (
        skills.map(s => (
          <div key={s.id} className="card">
            <div className="flex-between">
              <div>
                <h3>{s.name}</h3>
                <p>{s.description}</p>
                {s.version && <small>v{s.version}</small>}
                {s.source_url && <small style={{ display: 'block', color: 'var(--accent)' }}>{s.source_url}</small>}
              </div>
              <div style={{ display: 'flex', gap: 4, alignItems: 'center' }}>
                <label style={{ cursor: 'pointer', display: 'flex', alignItems: 'center', gap: 4, fontSize: 12 }}>
                  <input type="checkbox" checked={s.enabled} onChange={e => toggle(s.id, e.target.checked)} />
                  Enabled
                </label>
                <button className="btn btn-danger btn-sm" onClick={() => remove(s.id)}>✕</button>
              </div>
            </div>
          </div>
        ))
      )}

      {showImport && (
        <div className="modal-overlay open" onClick={() => setShowImport(false)}>
          <div className="modal" onClick={e => e.stopPropagation()}>
            <h2>Import Skill from URL</h2>
            <div className="form-group">
              <label>Skill URL (GitHub repo, OpenAPI spec, etc.)</label>
              <input value={importUrl} onChange={e => setImportUrl(e.target.value)}
                placeholder="https://github.com/owner/repo" autoFocus
                onKeyDown={e => e.key === 'Enter' && doImport()} />
            </div>
            <div className="form-actions">
              <button className="btn btn-outline" onClick={() => setShowImport(false)}>Cancel</button>
              <button className="btn btn-primary" onClick={doImport} disabled={!importUrl.trim()}>Import</button>
            </div>
          </div>
        </div>
      )}
    </div>
  )
}
