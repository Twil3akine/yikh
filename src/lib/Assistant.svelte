<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { invoke } from './api';
  import type { ChatMessage, AssistantSettings } from './api';
  let { active, focusToken }: { active: boolean; focusToken: number } = $props();
  let input: HTMLTextAreaElement;
  let conversation: HTMLDivElement;
  let messages = $state<ChatMessage[]>([]);
  let draft = $state('');
  let pending = $state(false);
  let error = $state('');
  let baseUrl = $state('http://127.0.0.1:8000/v1');
  let savingSettings = $state(false);
  let settingsError = $state('');
  let saved = $state(false);
  onMount(async () => {
    try { baseUrl = (await invoke<AssistantSettings>('get_assistant_settings')).base_url; }
    catch (cause) { settingsError = String(cause); }
  });
  $effect(() => { if (active && focusToken > 0) void tick().then(() => input?.focus()); });
  async function scrollToEnd() { await tick(); conversation.scrollTop = conversation.scrollHeight; }
  async function send(event?: SubmitEvent) {
    event?.preventDefault();
    const message = draft.trim();
    if (!message || pending) return;
    const history = messages.slice();
    messages = [...history, { role: 'user', content: message }];
    draft = ''; pending = true; error = ''; void scrollToEnd();
    try {
      const reply = await invoke<string>('ask_assistant', { message, history: history.slice(-20) });
      messages = [...messages, { role: 'assistant', content: reply }];
    } catch (cause) { messages = history; draft = message; error = String(cause); }
    finally { pending = false; void scrollToEnd(); if (active) input.focus(); }
  }
  async function saveSettings(event: SubmitEvent) {
    event.preventDefault(); savingSettings = true; settingsError = ''; saved = false;
    try {
      baseUrl = (await invoke<AssistantSettings>('save_assistant_settings', { settings: { base_url: baseUrl } })).base_url;
      saved = true;
    } catch (cause) { settingsError = String(cause); }
    finally { savingSettings = false; }
  }
</script>

<div class="assistant">
  <div class="connection"><span>Ornith 1.5 9B · ローカル</span>
    <details><summary>接続設定</summary>
      <form onsubmit={saveSettings} class="settings">
        <label>接続先<input type="url" bind:value={baseUrl} required oninput={() => saved = false} /></label>
        <button disabled={savingSettings || pending}>{savingSettings ? '保存中…' : '保存'}</button>
        {#if saved}<p class="muted" role="status">保存しました。</p>{/if}
      </form>
      {#if settingsError}<p class="error" role="alert">{settingsError}</p>{/if}
    </details>
  </div>
  <div class="conversation" bind:this={conversation} role="log" aria-label="Assistantとの会話" aria-live="polite">
    {#if messages.length === 0}<div class="welcome">
      <h2>何から進めますか？</h2>
      <p>保存されたアイテムをもとに、検索・整理や次に進めることを相談できます。</p>
      <p class="muted">「今週締切のTaskは？」<br />「Rustorchって今どうなってる？」</p>
    </div>{/if}
    {#each messages as message, index (index)}<article class:user={message.role === 'user'}>
      <h3>{message.role === 'user' ? 'あなた' : 'Assistant'}</h3><p>{message.content}</p>
    </article>{/each}
    {#if pending}<p class="muted waiting" role="status">回答を待っています…</p>{/if}
  </div>
  <div class="compose">
    {#if error}<p class="error" role="alert">{error}</p>{/if}
    <form onsubmit={send}>
      <textarea bind:this={input} bind:value={draft} aria-label="Assistantへの質問" placeholder="アイテムについて質問する" rows="3" disabled={pending}
        onkeydown={(event) => {
          if (event.key === 'Enter' && !event.shiftKey && !event.isComposing) { event.preventDefault(); void send(); }
        }}></textarea>
      <div class="send-row"><span class="muted">Enterで送信 · Shift + Enterで改行</span><button class="primary" disabled={pending || !draft.trim()}>送信</button></div>
    </form>
  </div>
</div>

<style>
  .assistant { width: 100%; height: 100%; min-width: 0; display: flex; flex-direction: column; min-height: 0; }
  .connection { min-width: 0; padding: 14px; border-bottom: 1px solid #e8eaec; color: #697178; font-size: 12px; overflow-wrap: anywhere; }
  details { min-width: 0; margin-top: 10px; }
  summary { cursor: pointer; width: fit-content; }
  .settings { min-width: 0; margin-top: 12px; display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: 8px; align-items: end; }
  .settings label { min-width: 0; display: grid; gap: 7px; }
  .settings input { width: 100%; min-width: 0; box-sizing: border-box; }
  .settings button { min-width: 0; }
  .settings p { width: 100%; margin: 0; }
  .conversation { flex: 1; min-width: 0; overflow: auto; padding: 16px 14px; }
  .welcome { min-width: 0; margin: 20px 0; max-width: 380px; }
  h2 { font-size: 21px; font-weight: 550; }
  .welcome p { font-size: 14px; line-height: 1.9; }
  article { min-width: 0; padding: 12px 0 20px; border-bottom: 1px solid #eceef0; margin-bottom: 14px; }
  article.user h3 { color: #737b82; }
  h3 { font-size: 12px; font-weight: 550; color: #303941; margin: 0 0 10px; }
  article p { white-space: pre-wrap; overflow-wrap: anywhere; font-size: 14px; line-height: 1.85; margin: 0; }
  .waiting { font-size: 13px; }
  .compose { min-width: 0; padding: 14px; border-top: 1px solid #e8eaec; }
  .compose form { min-width: 0; }
  .compose textarea { width: 100%; min-width: 0; box-sizing: border-box; resize: vertical; max-height: 160px; }
  .send-row { min-width: 0; display: flex; flex-wrap: wrap; justify-content: space-between; align-items: center; margin-top: 10px; gap: 8px; }
  .send-row span { min-width: 0; flex: 1 1 120px; font-size: 11px; }
  .compose :global(.error), .connection :global(.error) { overflow-wrap: anywhere; }
</style>
