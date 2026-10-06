<script lang="ts">
  import { onMount } from 'svelte';
  import { dateTime, invoke, priorityLabels } from './lib/api';
  import type { Catalog, Item, ItemInput, ItemKind } from './lib/api';
  import ItemEditor from './lib/ItemEditor.svelte';
  import Assistant from './lib/Assistant.svelte';
  import './app.css';
  let filter = $state<'all' | ItemKind>('all');
  let panel = $state<'details' | 'assistant'>('details');
  let items = $state<Item[]>([]);
  let selectedId = $state<string | null>(null);
  let selected = $derived(items.find((item) => item.id === selectedId));
  let includeCompleted = $state(false);
  let loading = $state(true);
  let working = $state(false);
  let error = $state('');
  let editing = $state(false);
  let editItem = $state<Item | null>(null);
  let catalog = $state<Catalog>({ projects: [], tags: [] });
  let deleteTarget = $state<Item | null>(null);
  let deleteDialog: HTMLDialogElement;
  let focusToken = $state(0);
  let requestId = 0;
  onMount(() => { void load(); });
  async function load(preferId?: string) {
    const id = ++requestId;
    loading = true; error = '';
    try {
      const result = await invoke<Item[]>('list_items', { query: {
        kind: filter === 'all' ? null : filter, status: includeCompleted ? null : 'active',
      } });
      if (id !== requestId) return;
      items = result;
      const candidate = preferId ?? selectedId;
      selectedId = result.some((item) => item.id === candidate) ? candidate : result[0]?.id ?? null;
    } catch (cause) { if (id === requestId) error = String(cause); }
    finally { if (id === requestId) loading = false; }
  }
  function changeFilter(next: 'all' | ItemKind) { filter = next; void load(); }
  function openAssistant() { panel = 'assistant'; focusToken += 1; }
  function keyboard(event: KeyboardEvent) {
    if (event.metaKey && event.key.toLowerCase() === 'k' && !event.isComposing) {
      event.preventDefault();
      if (!editing && !deleteDialog.open) openAssistant();
    }
  }
  async function openEditor(item: Item | null) {
    error = '';
    try {
      catalog = await invoke<Catalog>('item_catalog'); editItem = item; editing = true;
    } catch (cause) { error = String(cause); }
  }
  async function save(input: ItemInput) {
    const saved = editItem
      ? await invoke<Item>('update_item', { id: editItem.id, input })
      : await invoke<Item>('create_item', { input });
    panel = 'details'; await load(saved.id);
  }
  async function complete(item: Item) {
    working = true; error = '';
    try { await invoke<Item>('complete_item', { id: item.id }); await load(); }
    catch (cause) { error = String(cause); }
    finally { working = false; }
  }
  function confirmDelete(item: Item) { deleteTarget = item; deleteDialog.showModal(); }
  async function remove() {
    if (!deleteTarget || working) return;
    working = true; error = '';
    try {
      await invoke<void>('delete_item', { id: deleteTarget.id });
      deleteDialog.close(); deleteTarget = null; await load();
    } catch (cause) { error = String(cause); deleteDialog.close(); }
    finally { working = false; }
  }
</script>

<svelte:window onkeydown={keyboard} />
<div class="app">
  <header class="toolbar"><h1>Yikh</h1><button class="primary" onclick={() => openEditor(null)} disabled={working}>＋ 新規追加</button></header>
  {#if error}<div class="app-error" role="alert"><span>{error}</span><button onclick={() => load()}>再読み込み</button></div>{/if}
  <main>
    <section class="list-pane" aria-label="アイテム一覧">
      <nav class="tabs list-tabs" aria-label="アイテムの種類">
        {#each [['all', 'All'], ['task', 'Tasks'], ['bute', 'Butes']] as [value, label]}
          <button class:chosen={filter === value} aria-pressed={filter === value} onclick={() => changeFilter(value as 'all' | ItemKind)}>{label}</button>
        {/each}
      </nav>
      <div class="list-heading"><span>{filter === 'all' ? 'すべてのアイテム' : filter === 'task' ? 'Tasks' : 'Butes'}</span><span class="muted">{items.length}</span></div>
      <div class="item-list" aria-busy={loading}>
        {#if loading && items.length === 0}<p class="empty">読み込み中…</p>
        {:else if items.length === 0}<p class="empty">アイテムはありません。</p>
        {:else}{#each items as item (item.id)}
          <button class="item-row" class:selected={selectedId === item.id} class:completed={item.status === 'completed'} aria-pressed={selectedId === item.id}
            onclick={() => { selectedId = item.id; panel = 'details'; }}>
            <span class="item-title">{item.title}</span>
            <span class="item-meta"><span class="kind" class:bute={item.kind === 'bute'}>{item.kind === 'task' ? 'Task' : 'Bute'}</span>
              {#if item.project}<span class="project">{item.project}</span>{/if}
              <span class="item-date">{item.status === 'completed' ? '完了済み' : item.due_date ? `締切 ${item.due_date}` : `更新 ${dateTime(item.updated_at).split(' ')[0]}`}</span>
            </span>
          </button>
        {/each}{/if}
      </div>
      <footer class="list-footer"><label><input type="checkbox" checked={includeCompleted} onchange={(event) => { includeCompleted = event.currentTarget.checked; void load(); }} />完了済みも表示</label></footer>
    </section>
    <section class="right-pane" aria-label="詳細とAssistant">
      <nav class="tabs right-tabs" aria-label="右ペインの表示">
        <button class:chosen={panel === 'details'} aria-pressed={panel === 'details'} onclick={() => panel = 'details'}>Details</button>
        <button class:chosen={panel === 'assistant'} aria-pressed={panel === 'assistant'} onclick={openAssistant}>Assistant <span class="shortcut">⌘K</span></button>
      </nav>
      <div class="details-panel" hidden={panel !== 'details'}>
        {#if selected}
          <div class="detail-content">
            <div class="detail-kind"><span class="kind" class:bute={selected.kind === 'bute'}>{selected.kind === 'task' ? 'Task' : 'Bute'}</span>{#if selected.status === 'completed'}<span class="muted">完了済み</span>{/if}</div>
            <h2>{selected.title}</h2>
            <dl>
              <div><dt>プロジェクト</dt><dd>{selected.project ?? '未設定'}</dd></div>
              <div><dt>予定日</dt><dd>{selected.scheduled_date ?? '未設定'}</dd></div>
              <div><dt>締切日</dt><dd>{selected.due_date ?? '未設定'}</dd></div>
              <div><dt>優先度</dt><dd>{priorityLabels[selected.priority]}</dd></div>
              <div><dt>タグ</dt><dd>{selected.tags.length ? selected.tags.join('、') : '未設定'}</dd></div>
            </dl>
            <section class="notes"><h3>メモ</h3><p class:muted={!selected.notes}>{selected.notes || 'メモはありません。'}</p></section>
            <p class="timestamps muted">更新 {dateTime(selected.updated_at)}{#if selected.completed_at}<br />完了 {dateTime(selected.completed_at)}{/if}</p>
          </div>
          <footer class="detail-actions">
            <button class="primary" disabled={working || selected.status === 'completed'} onclick={() => selected && complete(selected)}>{selected.status === 'completed' ? '完了済み' : '完了'}</button>
            <button disabled={working} onclick={() => openEditor(selected ?? null)}>編集</button>
            <button class="danger delete-button" disabled={working} onclick={() => selected && confirmDelete(selected)}>削除</button>
          </footer>
        {:else}<div class="detail-empty"><p>アイテムを選択すると、詳細を表示します。</p></div>{/if}
      </div>
      <div class="assistant-panel" hidden={panel !== 'assistant'}><Assistant active={panel === 'assistant'} {focusToken} /></div>
    </section>
  </main>
</div>
{#if editing}<ItemEditor item={editItem} {catalog} onsave={save} onclose={() => editing = false} />{/if}
<dialog class="delete-dialog" bind:this={deleteDialog} aria-labelledby="delete-title" oncancel={(event) => { if (working) event.preventDefault(); }}>
  <h2 id="delete-title">アイテムを削除しますか？</h2>
  <p class="delete-name">{deleteTarget?.title}</p><p class="muted">削除したアイテムは元に戻せません。</p>
  <footer><button disabled={working} onclick={() => deleteDialog.close()}>キャンセル</button><button class="danger" disabled={working} onclick={remove}>削除</button></footer>
</dialog>
