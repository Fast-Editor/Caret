// Chat module public surface. The shell renders <ChatPane/> in the AI pane and <AgentsSidebar/> in the
// activity-bar "Agents" view; <AgentsLayout/> (components/agents) swaps the workbench when ui.layout === 'agents'.
export { ChatPane } from './ChatPane'
export { AgentsSidebar } from './AgentsSidebar'
export { initChatModule } from '@/lib/chatBridge'
export { Composer } from './Composer'
export { MessageList } from './MessageList'
export { ReviewBar } from './ReviewBar'
export { DiffView } from './DiffView'
