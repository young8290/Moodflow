import { commands, unwrap } from '@/api'
export type WorkspaceKind = 'chat' | 'inbox' | 'terminal'
export interface WorkspaceFile {
  id: string
  name: string
  bytes: number
  sha256: string
}
export interface WorkspaceMessage {
  id: string
  role: 'user' | 'assistant' | 'external' | 'note'
  body: string
  created_at: number
  files: WorkspaceFile[]
  reply_to: string | null
  mock: boolean
}
export interface WorkspaceSummary {
  id: string
  kind: WorkspaceKind
  title: string
  created_at: number
  updated_at: number
  archived: boolean
  unread: boolean
  count: number
  model: { endpoint: string; model: string } | null
  revision: string
}
export interface WorkspaceSession extends Omit<WorkspaceSummary, 'count' | 'revision'> {
  version: number
  instructions: string
  messages: WorkspaceMessage[]
}
export interface WorkspaceReply {
  session: WorkspaceSession
  revision: string
  mock?: boolean
}
export async function workspaceCall<T>(method: string, params: object = {}): Promise<T> {
  return JSON.parse(await unwrap(commands.workspaceRequest(method, JSON.stringify(params)))) as T
}
