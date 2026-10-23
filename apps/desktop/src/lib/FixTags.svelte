<script lang="ts">
  import { SvelteSet } from 'svelte/reactivity';

  import Icon from './Icon.svelte';
  import { attempt, type Backend, type Proposal } from './ipc';

  interface Props {
    api: Backend;
    /** One album, or the whole library when null. */
    albumId: number | null;
    /** What is checked: `Whole library` or the album's title. */
    scope: string;
    /** Label of the way back, e.g. `Settings`. */
    backLabel: string;
    onBack: () => void;
    onError: (message: string) => void;
  }

  const { api, albumId, scope, backLabel, onBack, onError }: Props = $props();

  /** Fixes listed at a time in a review. */
  const PAGE = 50;

  let proposals = $state<Proposal[] | null>(null);
  /** The kind of fix being reviewed, and how many of its fixes show. */
  let reviewing = $state<{ label: string; shown: number } | null>(null);
  /** Fixes unticked in the review. */
  const skipped = new SvelteSet<string>();
  let busy = $state(false);
  let applied = $state<string | null>(null);
  let undoable = $state(false);

  function keyOf(proposal: Proposal): string {
    return `${proposal.target.kind}:${String(proposal.target.id)}:${proposal.field}`;
  }

  /** A fix belongs to the last rule that changed it: the one that made
   * the visible difference (capitals, after spaces were trimmed). */
  function kindOf(proposal: Proposal): string {
    return proposal.rules.at(-1)?.label ?? '';
  }

  const kinds = $derived.by(() => {
    const byKind: [string, Proposal[]][] = [];

    for (const proposal of proposals ?? []) {
      const label = kindOf(proposal);
      const kind = byKind.find(([name]) => name === label);

      if (kind) kind[1].push(proposal);
      else byKind.push([label, [proposal]]);
    }

    return byKind;
  });

  const reviewed = $derived(
    kinds.find(([label]) => label === reviewing?.label)?.[1] ?? [],
  );
  const ticked = $derived(
    reviewed.filter((proposal) => !skipped.has(keyOf(proposal))),
  );

  async function load() {
    proposals = null;
    proposals = (await attempt(() => api.proposeFixes(albumId), onError)) ?? [];
  }

  $effect(() => {
    void load();
  });

  function shown(values: string[]): string {
    return values.length === 0 ? '(empty)' : values.join('; ');
  }

  function plural(count: number): string {
    return `${String(count)} ${count === 1 ? 'fix' : 'fixes'}`;
  }

  async function apply(fixes: Proposal[]) {
    busy = true;
    const done = await attempt(() => api.applyFixes(fixes), onError);

    busy = false;

    if (done !== null) return;

    applied = `Applied ${plural(fixes.length)}. Saved in Lyra; your music files are unchanged.`;
    undoable = true;
    reviewing = null;
    skipped.clear();
    await load();
  }

  async function undo() {
    undoable = false;
    const undone = await attempt(api.undoEdit, onError);

    applied = undone ? 'Undone.' : null;
    await load();
  }

  function review(label: string) {
    skipped.clear();
    reviewing = { label, shown: PAGE };
  }
</script>

<section class="fix" aria-labelledby="fix-title">
  <button type="button" class="back" onclick={onBack}
    ><Icon name="chevron-left" size={16} />{backLabel}</button
  >
  <div class="title">
    <h2 id="fix-title">Fix tags</h2>
    <p class="muted">{scope} · offline, saved in Lyra only</p>
  </div>

  {#if applied}
    <p class="confirm" role="status">
      {applied}
      {#if undoable}
        <button type="button" class="tertiary small" onclick={() => void undo()}
          >Undo</button
        >
      {/if}
    </p>
  {/if}

  {#if proposals === null}
    <p class="muted" role="status">Looking for fixes…</p>
  {:else if proposals.length === 0}
    <p class="muted" role="status">Nothing to fix.</p>
  {:else if reviewing === null}
    <ul class="panel kinds">
      {#each kinds as [label, fixes] (label)}
        {@const example = fixes[0]}
        <li class="setting">
          <div class="setting-text">
            <h4>{label}</h4>
            <p class="muted truncate">
              {plural(fixes.length)}{#if example}
                · {shown(example.before)} → {shown(example.after)}{/if}
            </p>
          </div>
          <div class="actions">
            <button
              type="button"
              class="small"
              onclick={() => {
                review(label);
              }}>Review</button
            >
            <button
              type="button"
              class="small"
              disabled={busy}
              onclick={() => void apply(fixes)}>Apply all</button
            >
          </div>
        </li>
      {/each}
    </ul>
  {:else}
    <button type="button" class="back" onclick={() => (reviewing = null)}
      ><Icon name="chevron-left" size={16} />All fixes</button
    >
    <h3 class="group-title">
      {reviewing.label} · {plural(reviewed.length)}
    </h3>
    <div class="panel">
      <table>
        <thead>
          <tr>
            <th class="tick"><span class="visually-hidden">Apply</span></th>
            <th>Item</th>
            <th class="what">Field</th>
            <th>Now</th>
            <th>Fixed</th>
          </tr>
        </thead>
        <tbody>
          {#each reviewed.slice(0, reviewing.shown) as fix (keyOf(fix))}
            <tr>
              <td class="tick">
                <input
                  type="checkbox"
                  aria-label={`Apply to ${fix.item}, ${fix.fieldLabel}`}
                  checked={!skipped.has(keyOf(fix))}
                  onchange={() => {
                    const key = keyOf(fix);

                    if (skipped.has(key)) skipped.delete(key);
                    else skipped.add(key);
                  }}
                />
              </td>
              <td title={fix.item}>{fix.item}</td>
              <td class="muted">{fix.fieldLabel}</td>
              <td class="muted" title={shown(fix.before)}
                >{shown(fix.before)}</td
              >
              <td title={shown(fix.after)}>{shown(fix.after)}</td>
            </tr>
          {/each}
        </tbody>
      </table>
      {#if reviewed.length > reviewing.shown}
        <button
          type="button"
          class="tertiary small more"
          onclick={() => {
            if (reviewing) reviewing.shown += PAGE;
          }}>Show more</button
        >
      {/if}
    </div>
    <div class="bar">
      <button
        type="button"
        class="primary"
        disabled={busy || ticked.length === 0}
        onclick={() => void apply(ticked)}>Apply {plural(ticked.length)}</button
      >
    </div>
  {/if}
</section>

<style>
  .fix {
    max-width: 960px;
  }
  .title {
    margin: var(--space-2) 0 var(--space-5);
  }
  .title h2 {
    font-size: var(--type-heading);
    line-height: var(--leading-heading);
    font-weight: var(--weight-strong);
  }
  .confirm {
    margin-bottom: var(--space-4);
  }
  .kinds {
    list-style: none;
    margin: 0;
  }
  table {
    width: 100%;
    border-collapse: collapse;
    table-layout: fixed;
    font-size: var(--type-small);
  }
  th {
    text-align: left;
    color: var(--text-secondary);
    font-weight: var(--weight-regular);
    font-size: var(--type-caption);
  }
  .tick {
    width: 2rem;
  }
  .what {
    width: 9rem;
  }
  th,
  td {
    padding: var(--space-2);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  tbody tr + tr td {
    border-top: 1px solid var(--line);
  }
  .more {
    margin: var(--space-2) 0 var(--space-3);
  }
  /* Pinned to the window's bottom edge, over the view's bottom padding. */
  .bar {
    position: sticky;
    bottom: calc(-1 * var(--space-6));
    z-index: 1;
    display: flex;
    justify-content: flex-end;
    margin: var(--space-4) 0 calc(-1 * var(--space-6));
    padding: var(--space-3) 0 var(--space-6);
    border-top: 1px solid var(--line);
    background: var(--bg);
  }
</style>
