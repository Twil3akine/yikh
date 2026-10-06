<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import type { Catalog, Item, ItemInput, ItemKind, Priority } from './api';
  import { priorityLabels } from './api';
  let { item, catalog, onsave, onclose }: {
    item: Item | null; catalog: Catalog; onsave: (input: ItemInput) => Promise<void>; onclose: () => void;
  } = $props();
  let dialog: HTMLDialogElement;
  let titleInput: HTMLInputElement;
  const initial = untrack(() => item);
  let kind = $state<ItemKind>(initial?.kind ?? 'task');
  let title = $state(initial?.title ?? '');
  let project = $state(initial?.project ?? '');
  let scheduled = $state(initial?.scheduled_date ?? '');
  let deadline = $state(initial?.due_date ?? '');
  let priority = $state<Priority>(initial?.priority ?? 'none');
  let tags = $state(initial?.tags.join(', ') ?? '');
  let notes = $state(initial?.notes ?? '');
  let saving = $state(false);
  let error = $state('');
  onMount(() => { dialog.showModal(); titleInput.focus(); });
  async function save(event: SubmitEvent) {
    event.preventDefault();
    if (saving) return;
    saving = true; error = '';
    try {
      await onsave({
        kind, title: title.trim(), project: project.trim() || null,
        scheduled_date: scheduled || null, due_date: deadline || null, priority,
        tags: tags.split(/[,、\n]/).map((tag) => tag.trim()).filter(Boolean), notes,
      });
      onclose();
    } catch (cause) { error = String(cause); }
    finally { saving = false; }
  }
</script>

<dialog bind:this={dialog} aria-labelledby="editor-title" oncancel={(event) => {
  event.preventDefault(); if (!saving) onclose();
}}>
  <form onsubmit={save}>
    <h2 id="editor-title">{item ? 'アイテムを編集' : '新規追加'}</h2>
    <fieldset disabled={saving}>
      <div class="form-row">
        <label>種類<select bind:value={kind}><option value="task">Task</option><option value="bute">Bute</option></select></label>
        <label>優先度<select bind:value={priority}>
          {#each Object.entries(priorityLabels) as [value, label]}<option {value}>{label}</option>{/each}
        </select></label>
      </div>
      <label>タイトル<input bind:this={titleInput} bind:value={title} required /></label>
      <label>プロジェクト<input bind:value={project} list="project-options" placeholder="任意" /></label>
      <datalist id="project-options">{#each catalog.projects as name}<option value={name}></option>{/each}</datalist>
      <div class="form-row">
        <label>予定日<input type="date" bind:value={scheduled} /></label>
        <label>締切日<input type="date" bind:value={deadline} /></label>
      </div>
      <label>タグ<input bind:value={tags} placeholder="カンマで区切って入力" /></label>
      <label>メモ<textarea bind:value={notes} rows="5"></textarea></label>
    </fieldset>
    {#if error}<p class="error" role="alert">{error}</p>{/if}
    <footer><button type="button" disabled={saving} onclick={onclose}>キャンセル</button>
      <button class="primary" disabled={saving || !title.trim()}>{saving ? '保存中…' : '保存'}</button></footer>
  </form>
</dialog>

<style>
  dialog { width: min(520px, calc(100vw - 64px)); padding: 26px; border: 1px solid #d9dddf; border-radius: 10px; max-height: calc(100vh - 64px); }
  dialog::backdrop { background: #0004; }
  h2 { font-size: 19px; font-weight: 600; margin: 0 0 22px; }
  fieldset { padding: 0; margin: 0; border: 0; display: grid; gap: 15px; }
  label { display: grid; gap: 7px; font-size: 13px; color: #4a5055; }
  .form-row { display: grid; grid-template-columns: 1fr 1fr; gap: 16px; }
  footer { display: flex; justify-content: flex-end; gap: 9px; margin-top: 23px; }
</style>
