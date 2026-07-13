import { useState, useEffect, useCallback } from 'react'
import './styles.css'
import { ProvidersPanel } from './components/ProvidersPanel'
import { AgentsPanel } from './components/AgentsPanel'
import { ChatArea } from './components/ChatArea'
import { SessionsPanel } from './components/SessionsPanel'
import { SkillsPanel } from './components/SkillsPanel'
import { ToolsPanel } from './components/ToolsPanel'
import { SettingsPanel } from './components/SettingsPanel'
import { SecurityPanel } from './components/SecurityPanel'
import { MemoryPanel } from './components/MemoryPanel'
import { ObservePanel } from './components/ObservePanel'
import { LogsPanel } from './components/LogsPanel'
import * as api from './api'
import type { AgentSummary, ProviderConfig, SessionSummary, ChatMessage, ToolBinding } from './types'

type Tab = 'agents' | 'providers' | 'skills' | 'tools' | 'settings' | 'security' | 'memory' | 'observe' | 'logs'

interface Toast { id: number; msg: string; type: 'error' | 'success' }
interface InboxItem { id: string; message: string; timestamp: string; read: boolean }

let toastId = 0;
let inboxId = 0;

export default function App() {
  const [tab, setTab] = useState<Tab>('agents')
  const [agents, setAgents] = useState<AgentSummary[]>([])
  const [providers, setProviders] = useState<ProviderConfig[]>([])
  const [selectedAgent, setSelectedAgent] = useState<AgentSummary | null>(null)
  const [sessions, setSessions] = useState<SessionSummary[]>([])
  const [currentSessionId, setCurrentSessionId] = useState<string | null>(null)
  const [messages, setMessages] = useState<ChatMessage[]>([])
  const [toasts, setToasts] = useState<Toast[]>([])
  const [inbox, setInbox] = useState<InboxItem[]>([])
  const [showInbox, setShowInbox] = useState(false)

  const addToast = useCallback((msg: string, type: 'error' | 'success' = 'error') => {
    const id = ++toastId
    setToasts(prev => [...prev, { id, msg, type }])
    setTimeout(() => setToasts(prev => prev.filter(t => t.id !== id)), 4000)
  }, [])

  const loadAgents = useCallback(async () => {
    try { setAgents(await api.listAgents()) }
    catch (e: any) { addToast(e.message) }
  }, [addToast])

  const loadProviders = useCallback(async () => {
    try { setProviders(await api.listProviders()) }
    catch (e: any) { addToast(e.message) }
  }, [addToast])

  const loadSessions = useCallback(async (agentId: string) => {
    try { setSessions(await api.listSessions(agentId)) }
    catch { /* ignore */ }
  }, [])

  const selectAgent = useCallback((agent: AgentSummary) => {
    setSelectedAgent(agent)
    setCurrentSessionId(null)
    setMessages([])
    loadSessions(agent.id)
  }, [loadSessions])

  useEffect(() => { loadProviders(); loadAgents() }, [loadProviders, loadAgents])

  const refreshAll = useCallback(() => {
    loadProviders(); loadAgents()
    if (selectedAgent) loadSessions(selectedAgent.id)
  }, [loadProviders, loadAgents, loadSessions, selectedAgent])

  return (
    <div className="app">
      {/* Sidebar */}
      <div className="sidebar">
        <div className="header">
          <h1>Praxis</h1>
          <span className="subtitle">Console</span>
          <div style={{ marginLeft: 'auto', position: 'relative' }}>
            <button className="btn btn-ghost btn-sm" onClick={() => setShowInbox(!showInbox)}
              style={{ position: 'relative', fontSize: 14 }}>
              🔔
              {inbox.filter(i => !i.read).length > 0 && (
                <span style={{
                  position: 'absolute', top: -2, right: -2,
                  background: 'var(--red)', color: '#fff',
                  borderRadius: '50%', width: 14, height: 14,
                  fontSize: 9, display: 'flex', alignItems: 'center', justifyContent: 'center',
                }}>{inbox.filter(i => !i.read).length}</span>
              )}
            </button>
            {showInbox && (
              <div style={{
                position: 'absolute', top: '100%', right: 0, marginTop: 4,
                width: 260, maxHeight: 200, overflowY: 'auto',
                background: 'var(--surface2)', border: '1px solid var(--border)',
                borderRadius: 'var(--radius)', boxShadow: 'var(--shadow)', zIndex: 50,
              }}>
                {inbox.length === 0 ? (
                  <div style={{ padding: 16, color: 'var(--text2)', fontSize: 12, textAlign: 'center' }}>
                    No notifications
                  </div>
                ) : (
                  inbox.map(item => (
                    <div key={item.id} style={{
                      padding: '8px 12px', fontSize: 12,
                      borderBottom: '1px solid var(--border)',
                      background: item.read ? 'transparent' : 'rgba(59,165,92,.05)',
                      cursor: 'pointer',
                    }} onClick={() => {
                      setInbox(prev => prev.map(i => i.id === item.id ? { ...i, read: true } : i))
                    }}>
                      <div>{item.message}</div>
                      <div style={{ color: 'var(--text2)', fontSize: 10, marginTop: 2 }}>{item.timestamp}</div>
                    </div>
                  ))
                )}
              </div>
            )}
          </div>
        </div>
        <div className="nav-tabs" style={{ flexWrap: 'wrap' }}>
          <div className={`nav-tab${tab === 'agents' ? ' active' : ''}`}
               onClick={() => setTab('agents')}>Agents</div>
          <div className={`nav-tab${tab === 'providers' ? ' active' : ''}`}
               onClick={() => setTab('providers')}>Prov</div>
          <div className={`nav-tab${tab === 'skills' ? ' active' : ''}`}
               onClick={() => setTab('skills')}>Skills</div>
          <div className={`nav-tab${tab === 'tools' ? ' active' : ''}`}
               onClick={() => setTab('tools')}>Tools</div>
          <div className={`nav-tab${tab === 'settings' ? ' active' : ''}`}
               onClick={() => setTab('settings')}>Settings</div>
          <div className={`nav-tab${tab === 'security' ? ' active' : ''}`}
               onClick={() => setTab('security')}>Security</div>
          <div className={`nav-tab${tab === 'memory' ? ' active' : ''}`}
               onClick={() => setTab('memory')}>Memory</div>
          <div className={`nav-tab${tab === 'observe' ? ' active' : ''}`}
               onClick={() => setTab('observe')}>Observe</div>
          <div className={`nav-tab${tab === 'logs' ? ' active' : ''}`}
               onClick={() => setTab('logs')}>Logs</div>
        </div>
        <div className={`tab-content${tab === 'agents' ? ' active' : ''}`}>
          <AgentsPanel
            agents={agents}
            providers={providers}
            selectedAgent={selectedAgent}
            onSelect={selectAgent}
            onRefresh={loadAgents}
            addToast={addToast}
          />
        </div>
        <div className={`tab-content${tab === 'providers' ? ' active' : ''}`}>
          <ProvidersPanel
            providers={providers}
            onRefresh={loadProviders}
            addToast={addToast}
          />
        </div>
        <div className={`tab-content${tab === 'skills' ? ' active' : ''}`}>
          <SkillsPanel addToast={addToast} />
        </div>
        <div className={`tab-content${tab === 'tools' ? ' active' : ''}`}>
          {selectedAgent ? (
            <ToolsPanel
              tools={selectedAgent ? ([] as ToolBinding[]) : []}
              onToolsChange={() => {}}
            />
          ) : (
            <div className="empty-state"><p>Select an agent to manage tools.</p></div>
          )}
        </div>
        <div className={`tab-content${tab === 'settings' ? ' active' : ''}`}>
          <SettingsPanel addToast={addToast} />
        </div>
        <div className={`tab-content${tab === 'security' ? ' active' : ''}`}>
          <SecurityPanel addToast={addToast} />
        </div>
        <div className={`tab-content${tab === 'memory' ? ' active' : ''}`}>
          <MemoryPanel addToast={addToast} />
        </div>
        <div className={`tab-content${tab === 'observe' ? ' active' : ''}`}>
          <ObservePanel addToast={addToast} />
        </div>
        <div className={`tab-content${tab === 'logs' ? ' active' : ''}`}>
          <LogsPanel addToast={addToast} />
        </div>
      </div>

      {/* Main area */}
      <div className="main">
        <div className="header flex-between">
          <div>
            <span id="active-agent-name">
              {selectedAgent ? selectedAgent.name : 'Select an agent'}
            </span>
            {selectedAgent && (
              <div className="subtitle">
                {providers.find(p => p.id === selectedAgent.provider_id)?.label || selectedAgent.provider_id}
                {' · '}{selectedAgent.tool_count} tools
              </div>
            )}
          </div>
          {selectedAgent && (
            <SessionsPanel
              sessions={sessions}
              currentSessionId={currentSessionId}
              onSelectSession={(id) => {
                setCurrentSessionId(id)
                setMessages([])
              }}
              onNewSession={() => {
                setCurrentSessionId(null)
                setMessages([])
              }}
              agentId={selectedAgent.id}
              onSessionsChange={() => loadSessions(selectedAgent.id)}
              addToast={addToast}
            />
          )}
        </div>

        {selectedAgent ? (
          <ChatArea
            key={selectedAgent.id}
            agentId={selectedAgent.id}
            sessionId={currentSessionId}
            messages={messages}
            onMessagesChange={setMessages}
            onSessionChange={(sid) => {
              setCurrentSessionId(sid)
              if (selectedAgent) loadSessions(selectedAgent.id)
            }}
            addToast={addToast}
          />
        ) : (
          <div className="empty-state" style={{flex:1,display:'flex',flexDirection:'column',justifyContent:'center',alignItems:'center'}}>
            <h3>Praxis Console</h3>
            <p>Select an agent from the sidebar to start chatting.</p>
            <p style={{ fontSize: 12, marginTop: 8, color: 'var(--text2)' }}>
              Manage agents, providers, skills, and settings via the sidebar tabs.
            </p>
          </div>
        )}

        {/* Toasts */}
        <div className="toast-container">
          {toasts.map(t => (
            <div key={t.id} className={`toast toast-${t.type}`}>{t.msg}</div>
          ))}
        </div>
      </div>
    </div>
  )
}
