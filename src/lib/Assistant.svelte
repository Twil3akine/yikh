<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { invoke } from './api';
  import type { Conversation, ConversationDetail, Message } from './api';
  import { renderMarkdown } from './markdown';
  import { createCompositionGuard, shouldSendOnEnter } from './composer';

  let { active, focusToken }: { active: boolean; focusToken: number } = $props();
  let input: HTMLTextAreaElement;
  let conversationView: HTMLDivElement;
  let conversations = $state<Conversation[]>([]);
  let current = $state<ConversationDetail | null>(null);
  let draft = $state('');
  let busy = $state(false);
  let sending = $state(false);
  let pendingQuestion = $state('');
  let error = $state('');
  let historyOpen = $state(false);
  let confirmDelete = $state(false);
  const composition = createCompositionGuard();
  const finalActionLabels = {
    create_item: 'この内容で追加',
    update_item: 'この内容で更新',
    complete_item: '完了にする',
    delete_item: 'このアイテムを削除',
  };

  $effect(() => { if (active && focusToken > 0) void tick().then(() => input?.focus()); });

  async function scrollToEnd() {
    await tick();
    if (conversationView) conversationView.scrollTop = conversationView.scrollHeight;
  }

  async function loadConversation(id: string) {
    current = await invoke<ConversationDetail>('get_conversation', { id });
    await scrollToEnd();
  }

  async function refreshConversations() {
    const listed = await invoke<Conversation[]>('list_conversations');
    conversations = listed.sort((left, right) => right.updated_at.localeCompare(left.updated_at));
  }

  onMount(async () => {
    busy = true;
    try {
      await refreshConversations();
      if (conversations.length) await loadConversation(conversations[0].id);
    } catch (cause) {
      error = String(cause);
    } finally {
      busy = false;
    }
  });

  async function createConversation() {
    if (busy) return;
    busy = true;
    error = '';
    confirmDelete = false;
    try {
      current = await invoke<ConversationDetail>('create_conversation');
      draft = '';
      await refreshConversations();
      historyOpen = false;
      await scrollToEnd();
      if (active) input?.focus();
    } catch (cause) {
      error = String(cause);
    } finally {
      busy = false;
    }
  }

  async function selectConversation(item: Conversation) {
    if (busy || item.id === current?.conversation.id) return;
    busy = true;
    error = '';
    confirmDelete = false;
    try {
      await loadConversation(item.id);
      draft = '';
      historyOpen = false;
    } catch (cause) {
      error = String(cause);
    } finally {
      busy = false;
      if (active) input?.focus();
    }
  }

  async function send(event?: SubmitEvent) {
    event?.preventDefault();
    const content = draft.trim();
    if (!content || busy || current?.pending_action) return;
    busy = true;
    error = '';
    confirmDelete = false;
    const previousIds = new Set(current?.messages.map((message) => message.id));
    try {
      if (!current) {
        current = await invoke<ConversationDetail>('create_conversation');
        await refreshConversations();
      }
      const id = current.conversation.id;
      draft = '';
      sending = true;
      pendingQuestion = content;
      void scrollToEnd();
      current = await invoke<ConversationDetail>('send_conversation_message', { id, content });
      pendingQuestion = '';
      await refreshConversations();
    } catch (cause) {
      error = String(cause);
      if (current) {
        try {
          current = await invoke<ConversationDetail>('get_conversation', { id: current.conversation.id });
          pendingQuestion = '';
          const persisted = current.messages.some((message) => !previousIds.has(message.id) && message.role === 'user' && message.content === content);
          if (!persisted) draft = content;
          await refreshConversations();
        } catch (reloadCause) {
          error = `${error}\n${String(reloadCause)}\n送信した質問: ${content}`;
        }
      }
      // Restore the draft only when the reload confirms it was not persisted.
    } finally {
      busy = false;
      sending = false;
      pendingQuestion = '';
      await scrollToEnd();
      if (active) input?.focus();
    }
  }

  async function deleteConversation() {
    if (busy || !current) return;
    busy = true;
    error = '';
    confirmDelete = false;
    const id = current.conversation.id;
    try {
      await invoke<void>('delete_conversation', { id });
      current = null;
      draft = '';
      await refreshConversations();
    } catch (cause) {
      error = String(cause);
    } finally {
      busy = false;
    }
  }

  function messageMarkup(message: Message) {
    return message.role === 'assistant' ? renderMarkdown(message.content) : '';
  }

  async function resolveItemAction(candidateKey: string | null, confirm: boolean) {
    if (busy || !current?.pending_action) return;
    const id = current.conversation.id;
    const token = current.pending_action.token;
    busy = true; error = '';
    try {
      current = await invoke<ConversationDetail>('resolve_assistant_action', { input: { id, token, candidate_key: candidateKey, confirm } });
      await refreshConversations();
    } catch (cause) {
      error = String(cause);
      try { await loadConversation(id); } catch { /* Keep the existing confirmation and error. */ }
    } finally {
      busy = false; await scrollToEnd();
      if (active) input?.focus();
    }
  }

  async function cancelItemAction() {
    if (busy || !current?.pending_action) return;
    busy = true; error = '';
    try {
      current = await invoke<ConversationDetail>('cancel_assistant_action', { id: current.conversation.id, token: current.pending_action.token });
      await refreshConversations();
    } catch (cause) { error = String(cause); }
    finally { busy = false; if (active) input?.focus(); }
  }

  function openLink(event: MouseEvent) {
    const link = (event.target as Element).closest('a');
    const url = link?.getAttribute('href');
    if (!url || url.startsWith('#')) return;
    event.preventDefault();
    void invoke<void>('open_assistant_link', { url }).catch((cause) => error = String(cause));
  }
</script>

<div class="assistant">
  <div class="assistant-toolbar">
    <button class="icon-button" aria-label="新しい会話" title="新しい会話" disabled={busy} onclick={createConversation}>＋</button>
    <button class="icon-button" aria-label="会話履歴" aria-expanded={historyOpen} title="会話履歴" disabled={busy} onclick={() => historyOpen = !historyOpen}>履歴</button>
    <div class="menu-wrap">
      <details>
        <summary aria-label="会話メニュー" title="会話メニュー">…</summary>
        <div class="menu">
          {#if confirmDelete}
            <span>この会話を削除しますか？</span>
            <button class="danger" disabled={busy || !current} onclick={deleteConversation}>削除</button>
            <button disabled={busy} onclick={() => confirmDelete = false}>キャンセル</button>
          {:else}
            <button disabled={busy || !current} onclick={() => confirmDelete = true}>現在の会話を削除</button>
          {/if}
        </div>
      </details>
    </div>
  </div>

  {#if historyOpen}
    <nav class="history" aria-label="会話履歴" aria-busy={busy}>
      {#if conversations.length === 0}<p class="muted">会話はありません。</p>{/if}
      {#each conversations as item (item.id)}
        <button class:chosen={item.id === current?.conversation.id} class="history-item" disabled={busy} onclick={() => selectConversation(item)}>
          <span>{item.title || '新しい会話'}</span>
          <time>{new Intl.DateTimeFormat('ja-JP', { month: 'numeric', day: 'numeric' }).format(new Date(item.updated_at))}</time>
        </button>
      {/each}
    </nav>
  {/if}

  <div class="conversation" bind:this={conversationView} role="log" aria-label="会話" aria-live="polite">
    {#if !current || current.messages.length === 0}
      <div class="welcome">
        <h2>何から進めますか？</h2>
        <p>保存されたアイテムをもとに、検索・整理や次に進めることを相談できます。</p>
        <p class="muted">「今週締切のTaskは？」</p>
      </div>
    {/if}
    {#if current}
      {#each current.messages as message (message.id)}
        <article class:user={message.role === 'user'}>
          {#if message.role === 'user' || message.content === current.pending_action?.message}
            <p>{message.content}</p>
          {:else}
            <!-- Markdown anchors already provide keyboard activation through click. -->
            <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
            <div class="markdown" onclick={openLink}>{@html messageMarkup(message)}</div>
          {/if}
        </article>
      {/each}
    {/if}
    {#if current?.pending_action}
      <section class="item-confirmation" aria-label="アイテム操作の確認">
        {#each current.pending_action.candidates as candidate (candidate.key)}
          {#if current.pending_action.kind === 'select'}
            <div class="candidate">
              <span class="candidate-title">{candidate.title}</span>
              <small>{candidate.kind === 'task' ? 'Task' : 'Bute'}{candidate.status === 'completed' ? '（完了済み）' : ''} / {candidate.project ?? 'プロジェクト未設定'} / 締切 {candidate.due_date ?? '未設定'}{#if candidate.scheduled_date} / 予定 {candidate.scheduled_date}{/if}</small>
              {#if candidate.notes}<small class="candidate-notes">{candidate.notes}</small>{/if}
              <button class="primary candidate-action" disabled={busy} aria-label={`${candidate.title}: 選択して内容確認へ進む`} onclick={() => resolveItemAction(candidate.key, false)}>
                選択して内容確認へ進む
              </button>
            </div>
          {:else}
            <p class="confirmation-target">{candidate.title}<small>{candidate.kind === 'task' ? 'Task' : 'Bute'}{candidate.status === 'completed' ? '（完了済み）' : ''} / {candidate.project ?? 'プロジェクト未設定'} / 締切 {candidate.due_date ?? '未設定'}</small>{#if candidate.notes}<small class="candidate-notes">{candidate.notes}</small>{/if}</p>
          {/if}
        {/each}
        <div class="confirmation-actions">
          {#if current.pending_action.kind === 'confirm' || current.pending_action.kind === 'delete'}
            <button class:danger={current.pending_action.operation === 'delete_item'} class:primary={current.pending_action.operation !== 'delete_item'} disabled={busy} onclick={() => resolveItemAction(null, true)}>{finalActionLabels[current.pending_action.operation]}</button>
          {/if}
          <button disabled={busy} onclick={cancelItemAction}>キャンセル</button>
        </div>
      </section>
    {/if}
    {#if sending && pendingQuestion}
      <article class="user"><p>{pendingQuestion}</p></article>
    {/if}
    {#if busy}<p class="muted waiting" role="status">{sending ? '回答を待っています…' : '読み込み中…'}</p>{/if}
  </div>

  <div class="compose">
    {#if error}<p class="error" role="alert">{error}</p>{/if}
    <form onsubmit={send}>
      <textarea bind:this={input} bind:value={draft} aria-label="質問" placeholder="アイテムについて質問する" rows="3" readonly={busy || !!current?.pending_action}
        oncompositionstart={() => composition.start()}
        oncompositionend={() => composition.end()}
        onkeydown={(event) => {
          if (shouldSendOnEnter(event, composition)) { event.preventDefault(); void send(); }
        }}></textarea>
      <div class="send-row"><span class="muted">Enterで送信 · Shift + Enterで改行</span><button class="primary" disabled={busy || !!current?.pending_action || !draft.trim()}>送信</button></div>
    </form>
  </div>
</div>

<style>
  .assistant { width: 100%; height: 100%; min-width: 0; display: flex; flex-direction: column; min-height: 0; }
  .assistant-toolbar { min-height: 46px; display: flex; align-items: center; gap: 5px; padding: 0 10px; border-bottom: 1px solid #e8eaec; }
  .icon-button, .menu-wrap summary { border: 0; background: transparent; border-radius: 5px; padding: 6px 9px; color: #657078; font-size: 12px; cursor: pointer; list-style: none; }
  .icon-button:first-child { font-size: 18px; }
  .icon-button:hover, .menu-wrap summary:hover { background: #f1f4f5; }
  .menu-wrap { position: relative; margin-left: auto; }
  .menu-wrap summary::-webkit-details-marker { display: none; }
  .menu { position: absolute; z-index: 2; right: 0; top: 28px; width: max-content; max-width: min(280px, 80vw); display: grid; gap: 6px; padding: 8px; background: white; border: 1px solid #dce1e3; border-radius: 7px; box-shadow: 0 5px 18px #0002; }
  .menu button { text-align: left; border: 0; padding: 7px 9px; font-size: 12px; }
  .menu span { padding: 4px 7px; font-size: 12px; }
  .history { max-height: 180px; flex-shrink: 0; overflow-y: auto; border-bottom: 1px solid #e8eaec; padding: 5px 8px; }
  .history > p { margin: 9px 7px; font-size: 12px; }
  .history-item { width: 100%; display: flex; align-items: center; gap: 8px; border: 0; border-radius: 4px; background: transparent; padding: 7px; text-align: left; font-size: 12px; }
  .history-item span { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .history-item time { flex: 0 0 auto; color: #8a9298; font-size: 10px; }
  .history-item:hover, .history-item.chosen { background: #edf2f4; }
  .conversation { flex: 1; min-width: 0; overflow: auto; padding: 16px 14px; }
  .welcome { min-width: 0; margin: 20px 0; max-width: 380px; }
  h2 { font-size: 21px; font-weight: 550; }
  .welcome p { font-size: 14px; line-height: 1.9; }
  article { min-width: 0; padding: 12px 0 20px; border-bottom: 1px solid #eceef0; margin-bottom: 14px; }
  article.user { color: #5f6870; }
  article p { white-space: pre-wrap; overflow-wrap: anywhere; font-size: 14px; line-height: 1.85; margin: 0; }
  .waiting { font-size: 13px; }
  .item-confirmation { display: grid; gap: 6px; margin: 12px 0; }
  .candidate { display: grid; gap: 5px; min-width: 0; text-align: left; padding: 10px; background: #fafbfc; border: 1px solid #d8dddf; border-radius: 6px; }
  .candidate-title { font-weight: 500; }
  .candidate-action { width: 100%; margin-top: 5px; padding: 8px 10px; white-space: normal; overflow-wrap: anywhere; }
  .candidate span, .confirmation-target { overflow-wrap: anywhere; font-size: 13px; }
  .item-confirmation small { display: block; color: #7c858c; font-size: 11px; line-height: 1.6; }
  .item-confirmation .candidate-notes { display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; overflow-wrap: anywhere; }
  .confirmation-target { margin: 0 0 4px; }
  .confirmation-actions { display: flex; flex-wrap: wrap; gap: 6px; }
  .compose { min-width: 0; padding: 14px; border-top: 1px solid #e8eaec; }
  .compose form { min-width: 0; }
  .compose textarea { width: 100%; min-width: 0; box-sizing: border-box; resize: vertical; max-height: 160px; }
  .send-row { min-width: 0; display: flex; flex-wrap: wrap; justify-content: space-between; align-items: center; margin-top: 10px; gap: 8px; }
  .send-row span { min-width: 0; flex: 1 1 120px; font-size: 11px; }
  .compose :global(.error) { white-space: pre-wrap; overflow-wrap: anywhere; }
  .markdown { min-width: 0; overflow-wrap: anywhere; font-size: 14px; line-height: 1.75; }
  .markdown :global(h1), .markdown :global(h2), .markdown :global(h3), .markdown :global(h4) { margin: 1em 0 .45em; font-size: 1.12em; line-height: 1.45; }
  .markdown :global(p) { margin: .55em 0; white-space: pre-wrap; }
  .markdown :global(ul), .markdown :global(ol) { padding-left: 1.5em; margin: .55em 0; }
  .markdown :global(li) { margin: .2em 0; }
  .markdown :global(code) { background: #f1f3f4; border-radius: 3px; padding: .12em .3em; font-family: ui-monospace, SFMono-Regular, monospace; font-size: .9em; overflow-wrap: anywhere; }
  .markdown :global(pre) { max-width: 100%; overflow: auto; padding: 10px; background: #f5f6f7; border-radius: 5px; }
  .markdown :global(pre code) { display: block; padding: 0; white-space: pre; overflow-wrap: normal; background: transparent; }
  .markdown :global(table) { display: block; max-width: 100%; overflow-x: auto; border-collapse: collapse; }
  .markdown :global(th), .markdown :global(td) { border: 1px solid #dfe3e5; padding: 5px 8px; text-align: left; }
  .markdown :global(a) { color: #3b697e; text-decoration: underline; overflow-wrap: anywhere; }
</style>
