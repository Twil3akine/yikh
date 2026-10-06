<script lang="ts">
  import { onMount } from 'svelte';
  import { dateTime, invoke, priorityLabels } from './lib/api';
  import type { Catalog, Item, ItemInput, ItemKind } from './lib/api';
  import ItemEditor from './lib/ItemEditor.svelte';
  import Assistant from './lib/Assistant.svelte';
  import './app.css';
  let filter = $state<'all' | ItemKind>('all');
  let leftOpen = $state(true);
  let assistantOpen = $state(true);
  let leftWidth = $state(260);
  let rightWidth = $state(280);
  let layoutWidth = $state(1060);
  let layout: HTMLElement;
  let listPane: HTMLElement;
  let detailPane: HTMLElement;
  let assistantPane: HTMLElement;
  type Sidebar = 'left' | 'right';
  const minimumWidths = { left: 200, right: 220 };
  const dividerWidth = 6;
  const detailMinimum = 300;
  let resizing = $state<{ side: Sidebar; startX: number; startWidth: number; pointerId: number } | null>(null);
  let items = $state<Item[]>([]);
  let activeItems = $state<Item[]>([]);
  let counts = $derived({
    all: activeItems.length,
    task: activeItems.filter((item) => item.kind === 'task').length,
    bute: activeItems.filter((item) => item.kind === 'bute').length,
  });
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
  onMount(() => { fitSidebars(); void load(); });

  function fitSidebars() {
    if (!layout) return;
    layoutWidth = layout.clientWidth;
    const total = (leftOpen ? leftWidth + dividerWidth : 0) + (assistantOpen ? rightWidth + dividerWidth : 0);
    const excess = Math.max(0, total + detailMinimum - layoutWidth);
    const leftSlack = leftOpen ? leftWidth - minimumWidths.left : 0;
    const rightSlack = assistantOpen ? rightWidth - minimumWidths.right : 0;
    const slack = leftSlack + rightSlack;
    if (excess > 0 && slack > 0) {
      const fraction = Math.min(1, excess / slack);
      if (leftOpen) leftWidth = Math.floor(leftWidth - leftSlack * fraction);
      if (assistantOpen) rightWidth = Math.floor(rightWidth - rightSlack * fraction);
    }
  }

  function maximumWidth(side: Sidebar) {
    const other = side === 'left'
      ? (assistantOpen ? rightWidth + dividerWidth : 0)
      : (leftOpen ? leftWidth + dividerWidth : 0);
    return Math.max(minimumWidths[side], Math.min(420, layoutWidth - detailMinimum - other - dividerWidth));
  }

  function setWidth(side: Sidebar, value: number) {
    const width = Math.max(minimumWidths[side], Math.min(maximumWidth(side), Math.round(value)));
    if (side === 'left') leftWidth = width;
    else rightWidth = width;
  }

  function startResize(event: PointerEvent, side: Sidebar) {
    if (event.button !== 0 || !event.isPrimary) return;
    event.preventDefault();
    const handle = event.currentTarget as HTMLElement;
    handle.focus();
    handle.setPointerCapture(event.pointerId);
    resizing = { side, startX: event.clientX, startWidth: side === 'left' ? leftWidth : rightWidth, pointerId: event.pointerId };
  }

  function resize(event: PointerEvent) {
    if (!resizing || event.pointerId !== resizing.pointerId) return;
    const delta = (event.clientX - resizing.startX) * (resizing.side === 'left' ? 1 : -1);
    setWidth(resizing.side, resizing.startWidth + delta);
  }

  function endResize(event: PointerEvent) {
    if (event.pointerId !== resizing?.pointerId) return;
    resizing = null;
    const handle = event.currentTarget as HTMLElement;
    if (handle.hasPointerCapture(event.pointerId)) handle.releasePointerCapture(event.pointerId);
  }

  function resizeKey(event: KeyboardEvent, side: Sidebar) {
    const current = side === 'left' ? leftWidth : rightWidth;
    const direction = side === 'left' ? 1 : -1;
    if (event.key === 'ArrowLeft') setWidth(side, current - 10 * direction);
    else if (event.key === 'ArrowRight') setWidth(side, current + 10 * direction);
    else if (event.key === 'Home') setWidth(side, minimumWidths[side]);
    else if (event.key === 'End') setWidth(side, maximumWidth(side));
    else return;
    event.preventDefault();
  }

  function toggleLeft() {
    if (leftOpen && listPane.contains(document.activeElement)) detailPane.focus();
    leftOpen = !leftOpen;
    resizing = null;
    fitSidebars();
  }

  function closeAssistant() {
    if (assistantPane.contains(document.activeElement)) detailPane.focus();
    assistantOpen = false;
    resizing = null;
  }
  async function load(preferId?: string) {
    const id = ++requestId;
    loading = true; error = '';
    try {
      const activeRequest = invoke<Item[]>('list_items', { query: { status: 'active' } });
      const visibleRequest = filter === 'all' && !includeCompleted ? activeRequest : invoke<Item[]>('list_items', { query: {
        kind: filter === 'all' ? null : filter, status: includeCompleted ? null : 'active',
      } });
      const [result, active] = await Promise.all([visibleRequest, activeRequest]);
      if (id !== requestId) return;
      items = result;
      activeItems = active;
      const candidate = preferId ?? selectedId;
      selectedId = result.some((item) => item.id === candidate) ? candidate : result[0]?.id ?? null;
    } catch (cause) { if (id === requestId) error = String(cause); }
    finally { if (id === requestId) loading = false; }
  }
  function changeFilter(next: 'all' | ItemKind) { filter = next; void load(); }
  function openAssistant() { assistantOpen = true; fitSidebars(); focusToken += 1; }
  function keyboard(event: KeyboardEvent) {
    if (!event.metaKey || event.altKey || event.ctrlKey || event.shiftKey || event.isComposing || editing || deleteDialog.open) return;
    const key = event.key.toLowerCase();
    if (key !== 'b' && key !== 'k') return;
    event.preventDefault();
    if (key === 'b') toggleLeft();
    else openAssistant();
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
    await load(saved.id);
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

<svelte:window onkeydown={keyboard} onresize={fitSidebars} />
<div class="app">
  <header class="toolbar">
    <div class="toolbar-group"><h1>Yikh</h1>
      <button class="sidebar-toggle" aria-expanded={leftOpen} aria-controls="item-sidebar" aria-label={leftOpen ? 'アイテム一覧を閉じる' : 'アイテム一覧を開く'} onclick={toggleLeft}>一覧 <span class="shortcut">⌘B</span></button>
    </div>
    <div class="toolbar-group">
      <button class="sidebar-toggle" aria-expanded={assistantOpen} aria-controls="assistant-sidebar" onclick={openAssistant}>Assistant <span class="shortcut">⌘K</span></button>
      <button class="primary" onclick={() => openEditor(null)} disabled={working}>＋ 新規追加</button>
    </div>
  </header>
  {#if error}<div class="app-error" role="alert"><span>{error}</span><button onclick={() => load()}>再読み込み</button></div>{/if}
  <main bind:this={layout} class:resizing={resizing !== null} style={`--left-width: ${leftWidth}px; --right-width: ${rightWidth}px;`}>
    <section id="item-sidebar" class="list-pane" aria-label="アイテム一覧" hidden={!leftOpen} bind:this={listPane}>
      <nav class="tabs list-tabs" aria-label="アイテムの種類">
        {#each [['all', 'All'], ['task', 'Tasks'], ['bute', 'Butes']] as [value, label]}
          <button class:chosen={filter === value} aria-pressed={filter === value} onclick={() => changeFilter(value as 'all' | ItemKind)}><span>{label}</span><span class="count-badge" aria-label="未完了件数">{counts[value as 'all' | ItemKind]}</span></button>
        {/each}
      </nav>
      <div class="item-list" aria-busy={loading}>
        {#if loading && items.length === 0}<p class="empty">読み込み中…</p>
        {:else if items.length === 0}<p class="empty">アイテムはありません。</p>
        {:else}{#each items as item (item.id)}
          <button class="item-row" class:selected={selectedId === item.id} class:completed={item.status === 'completed'} aria-pressed={selectedId === item.id}
            onclick={() => selectedId = item.id}>
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
    {#if leftOpen}
      <!-- Focusable separators follow the ARIA window splitter pattern. -->
      <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
      <div class="sidebar-divider" role="separator" tabindex="0" aria-label="アイテム一覧の幅" aria-orientation="vertical" aria-controls="item-sidebar"
        aria-valuemin={minimumWidths.left} aria-valuemax={maximumWidth('left')} aria-valuenow={leftWidth}
        onpointerdown={(event) => startResize(event, 'left')} onpointermove={resize} onpointerup={endResize} onpointercancel={endResize} onlostpointercapture={endResize}
        onkeydown={(event) => resizeKey(event, 'left')}></div>
    {/if}
    <section class="main-pane" aria-label="アイテム詳細" tabindex="-1" bind:this={detailPane}>
      <div class="details-panel">
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
    </section>
    {#if assistantOpen}
      <!-- Focusable separators follow the ARIA window splitter pattern. -->
      <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
      <div class="sidebar-divider" role="separator" tabindex="0" aria-label="Assistantの幅" aria-orientation="vertical" aria-controls="assistant-sidebar"
        aria-valuemin={minimumWidths.right} aria-valuemax={maximumWidth('right')} aria-valuenow={rightWidth}
        onpointerdown={(event) => startResize(event, 'right')} onpointermove={resize} onpointerup={endResize} onpointercancel={endResize} onlostpointercapture={endResize}
        onkeydown={(event) => resizeKey(event, 'right')}></div>
    {/if}
    <section id="assistant-sidebar" class="assistant-pane" aria-label="Assistant" hidden={!assistantOpen} bind:this={assistantPane}>
      <header class="sidebar-header"><span>Assistant</span><button class="close-sidebar" aria-label="Assistantを閉じる" onclick={closeAssistant}>閉じる</button></header>
      <div class="assistant-panel"><Assistant active={assistantOpen} {focusToken} /></div>
    </section>
  </main>
</div>
{#if editing}<ItemEditor item={editItem} {catalog} onsave={save} onclose={() => editing = false} />{/if}
<dialog class="delete-dialog" bind:this={deleteDialog} aria-labelledby="delete-title" oncancel={(event) => { if (working) event.preventDefault(); }}>
  <h2 id="delete-title">アイテムを削除しますか？</h2>
  <p class="delete-name">{deleteTarget?.title}</p><p class="muted">削除したアイテムは元に戻せません。</p>
  <footer><button disabled={working} onclick={() => deleteDialog.close()}>キャンセル</button><button class="danger" disabled={working} onclick={remove}>削除</button></footer>
</dialog>
