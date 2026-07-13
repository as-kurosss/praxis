import { useState, useEffect, useCallback } from 'react'
import './styles.css'
import { ProvidersPanel } from './components/ProvidersPanel'
import { AgentsPanel } from './components/AgentsPanel'
import { ChatArea } from './components/ChatArea'
import { SessionsPanel } from './components/SessionsPanel'
import { ObservePage } from './components/observe/ObservePage'
import * as api from './api'
import type { AgentSummary, ProviderConfig, SessionSummary, ChatMessage } from './types'

type Tab = 'agents' | 'providers' | 'observe'

interface Toast { id: number; msg: string; type: 'error' | 'success' }

let toastId = 0;

export default function App() {
  const [tab, setTab] = useState<Tab>('agents')
  const [agents, setAgents] = useState<AgentSummary[]>([])
  const [providers, setProviders] = useState<ProviderConfig[]>([])
  const [selectedAgent, setSelectedAgent] = useState<AgentSummary | null>(null)
  const [sessions, setSessions] = useState<SessionSummary[]>([])
  const [currentSessionId, setCurrentSessionId] = useState<string | null>(null)
  const [messages, setMessages] = useState<ChatMessage[]>([])
  const [toasts, setToasts] = useState<Toast[]>([])

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
        </div>
        <div className="nav-tabs">
          <div className={`nav-tab${tab === 'agents' ? ' active' : ''}`}
               onClick={() => setTab('agents')}>Agents</div>
          <div className={`nav-tab${tab === 'providers' ? ' active' : ''}`}
               onClick={() => setTab('providers')}>Providers</div>
          <div className={`nav-tab${tab === 'observe' ? ' active' : ''}`}
               onClick={() => setTab('observe')}>Observe</div>
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
      </div>

      {/* Main area */}
      <div className="main" style={tab === 'observe' ? { overflow: 'auto' } : undefined}>
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
        ) : tab === 'observe' ? (
          <ObservePage addToast={addToast} />
        ) : (
          <div className="empty-state" style={{flex:1,display:'flex',flexDirection:'column',justifyContent:'center'}}>
            <h3>Praxis Console</h3>
            <p>Select an agent from the sidebar to start chatting.</p>
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
