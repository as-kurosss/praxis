import type { Config } from '../types'

interface Props {
  config: Config | null
  viewMode: 'normal' | 'wide' | 'simple'
  onViewModeChange: (mode: 'normal' | 'wide' | 'simple') => void
  onClose: () => void
}

export function SettingsPanel({ config, viewMode, onViewModeChange, onClose }: Props) {
  return (
    <div className="modal-overlay open" onClick={onClose}>
      <div className="modal settings-modal" onClick={e => e.stopPropagation()}>
        <div className="flex-between">
          <h2>Settings</h2>
          <button className="btn btn-ghost btn-sm" onClick={onClose}>✕</button>
        </div>

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
