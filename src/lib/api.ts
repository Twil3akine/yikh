export type ItemKind = 'task' | 'bute';
export type ItemStatus = 'active' | 'completed';
export type Priority = 'none' | 'low' | 'medium' | 'high';
export interface ItemInput {
  kind: ItemKind; title: string; notes: string; project: string | null;
  scheduled_date: string | null; due_date: string | null; priority: Priority; tags: string[];
}
export interface Item extends ItemInput {
  id: string; status: ItemStatus; created_at: string; updated_at: string; completed_at: string | null;
}
export interface Catalog { projects: string[]; tags: string[] }
export interface ChatMessage { role: 'user' | 'assistant'; content: string }
export interface Conversation { id: string; title: string; created_at: string; updated_at: string }
export interface Message extends ChatMessage { id: string; conversation_id: string; created_at: string }
export interface ActionCandidate {
  key: string; title: string; kind: ItemKind; project: string | null;
  scheduled_date: string | null; due_date: string | null; priority: Priority;
  status: ItemStatus; notes: string;
}
export interface PendingAction {
  token: string; kind: 'select' | 'delete'; operation: 'update_item' | 'complete_item' | 'delete_item';
  message: string; candidates: ActionCandidate[];
}
export interface ConversationDetail { conversation: Conversation; messages: Message[]; pending_action?: PendingAction | null }
export interface AssistantSettings { base_url: string }
declare global {
  interface Window {
    __TAURI__?: {
      core: { invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> };
      event: { listen(event: string, handler: () => void): Promise<() => void> };
    };
  }
}
export function listenForItemChanges(refresh: () => void): Promise<() => void> {
  if (!window.__TAURI__) return Promise.reject('デスクトップアプリから開いてください。');
  return window.__TAURI__.event.listen('items-changed', refresh);
}
// Tauri injects this bridge. Persistence and model requests stay in Rust.
export function invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!window.__TAURI__) return Promise.reject('デスクトップアプリから開いてください。');
  return window.__TAURI__.core.invoke<T>(command, args);
}
export const priorityLabels: Record<Priority, string> = {
  none: '未設定', low: '低', medium: '中', high: '高',
};
export function dateTime(value: string): string {
  return new Intl.DateTimeFormat('ja-JP', {
    year: 'numeric', month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit',
  }).format(new Date(value));
}
