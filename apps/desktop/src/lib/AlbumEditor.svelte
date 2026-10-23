<script lang="ts">
  import { onMount } from 'svelte';
  import { SvelteMap, SvelteSet } from 'svelte/reactivity';

  import FieldGrid from './FieldGrid.svelte';
  import FieldInput from './FieldInput.svelte';
  import Icon from './Icon.svelte';
  import {
    BackendError,
    attempt,
    type Album,
    type AlbumEditor,
    type Backend,
    type FieldChange,
    type FieldInfo,
    type TargetRef,
  } from './ipc';

  interface Props {
    api: Backend;
    album: Album;
    /** Leaves without saving. */
    onBack: () => void;
    /** After changes were saved. */
    onSaved: () => void;
    onError: (message: string) => void;
  }

  const { api, album, onBack, onSaved, onError }: Props = $props();

  /** Album fields shown first; the rest are under More fields. */
  const MAIN = ['album', 'album_artist', 'date', 'compilation'];
  /** Track fields in the table; the rest open per track under More fields. */
  const COLUMNS = ['disc', 'track', 'title', 'artist'];
  /** The album's genre: typed once, set on every track. */
  const GENRE = 'album:genre';

  let data = $state<AlbumEditor | null>(null);
  /** What was typed, by field key; untouched fields are absent. */
  const drafts = new SvelteMap<string, string>();
  /** Edited fields to show from the files again. */
  const resets = new SvelteSet<string>();
  let more = $state(false);
  /** The track whose other fields are open. */
  let expanded = $state<number | null>(null);
  let saving = $state(false);
  let problem = $state<string | null>(null);
  /** Back was pressed with unsaved changes. */
  let leaving = $state(false);

  const release = $derived<TargetRef>({ kind: 'release', id: album.id });

  function track(id: number): TargetRef {
    return { kind: 'track', id };
  }

  function keyOf(target: TargetRef, field: string): string {
    return `${target.kind}:${String(target.id)}:${field}`;
  }

  /** Several values are typed separated by `;`. */
  function initial(info: FieldInfo): string {
    return info.values.join('; ');
  }

  function field(fields: FieldInfo[], name: string): FieldInfo | undefined {
    return fields.find((info) => info.field === name);
  }

  /** The distinct values the tracks give `name`. */
  function trackValues(name: string): Set<string> {
    return new Set(
      (data?.tracks ?? []).map((t) => {
        const info = field(t.fields, name);

        return info ? initial(info) : '';
      }),
    );
  }

  /** The tracks' genre when they all have the same; null when they differ. */
  const genre = $derived.by(() => {
    const genres = trackValues('genre');

    return genres.size === 1 ? ([...genres][0] ?? '') : null;
  });
  /** The disc column shows when the tracks are on several discs. */
  const discs = $derived(trackValues('disc').size > 1);

  function current(target: TargetRef, info: FieldInfo): string {
    return drafts.get(keyOf(target, info.field)) ?? initial(info);
  }

  function changesOf(target: TargetRef, fields: FieldInfo[]): FieldChange[] {
    const changes: FieldChange[] = [];

    for (const info of fields) {
      const key = keyOf(target, info.field);

      const draft =
        drafts.get(key) ??
        (target.kind === 'track' && info.field === 'genre'
          ? drafts.get(GENRE)
          : undefined);

      if (resets.has(key)) {
        changes.push({ target, field: info.field, edit: { kind: 'reset' } });
      } else if (draft !== undefined && draft !== initial(info)) {
        const values = (info.multiple ? draft.split(';') : [draft])
          .map((value) => value.trim())
          .filter((value) => value !== '');

        changes.push({
          target,
          field: info.field,
          edit:
            values.length === 0 ? { kind: 'clear' } : { kind: 'set', values },
        });
      }
    }

    return changes;
  }

  const changes = $derived(
    data === null
      ? []
      : [
          ...changesOf(release, data.fields),
          ...data.tracks.flatMap((t) => changesOf(track(t.id), t.fields)),
        ],
  );

  async function load() {
    const next = await attempt(() => api.albumEditor(album.id), onError);

    if (next) data = next;
    else onBack();
  }

  async function save() {
    if (data === null) return;

    saving = true;
    problem = null;

    try {
      await api.saveAlbumEdits(changes, data.revisions);
      onSaved();
    } catch (error) {
      if (error instanceof BackendError && error.code === 'changed_meanwhile') {
        problem =
          'The album changed while you were editing. Check and save again.';
        await load();
      } else {
        problem =
          error instanceof BackendError ? error.message : 'Could not save.';
      }
    } finally {
      saving = false;
    }
  }

  function back() {
    if (changes.length > 0) leaving = true;
    else onBack();
  }

  function toggleReset(key: string) {
    if (resets.has(key)) {
      resets.delete(key);
    } else {
      resets.add(key);
      drafts.delete(key);
    }
  }

  onMount(() => {
    void load();
  });
</script>

<section class="editor" aria-labelledby="editor-title">
  <button type="button" class="back" onclick={back}
    ><Icon name="chevron-left" size={16} />Back to album</button
  >
  <header>
    {#if data?.art}
      <img class="art" src={data.art} alt="" width="96" height="96" />
    {:else}
      <div class="art" aria-hidden="true"><Icon name="disc-3" size={40} /></div>
    {/if}
    <div class="heading-text">
      <h2 id="editor-title">Edit album</h2>
      <p class="muted">{album.title} · saved in Lyra, files unchanged</p>
    </div>
  </header>

  {#if data === null}
    <p class="muted" role="status">Loading…</p>
  {:else}
    <FieldGrid
      fields={data.fields.filter((info) => MAIN.includes(info.field))}
      valueOf={(info: FieldInfo) => current(release, info)}
      isReset={(info: FieldInfo) => resets.has(keyOf(release, info.field))}
      onInput={(info: FieldInfo, value: string) => {
        drafts.set(keyOf(release, info.field), value);
      }}
      onToggleReset={(info: FieldInfo) => {
        toggleReset(keyOf(release, info.field));
      }}
    />
    <label class="field genre">
      <span class="label">Genre</span>
      <input
        type="text"
        value={drafts.get(GENRE) ?? genre ?? ''}
        placeholder={genre === null
          ? 'Several genres: type one to set it on every track'
          : 'Separate genres with ;'}
        oninput={(event) => {
          drafts.set(GENRE, event.currentTarget.value);
        }}
      />
    </label>

    <button
      type="button"
      class="tertiary small with-icon more-toggle"
      aria-expanded={more}
      onclick={() => (more = !more)}
      ><Icon name="chevron-down" size={14} />{more
        ? 'Fewer fields'
        : 'More fields'}</button
    >
    {#if more}
      <p class="muted hint">Sort names, totals and MusicBrainz IDs.</p>
      <FieldGrid
        fields={data.fields.filter((info) => !MAIN.includes(info.field))}
        valueOf={(info: FieldInfo) => current(release, info)}
        isReset={(info: FieldInfo) => resets.has(keyOf(release, info.field))}
        onInput={(info: FieldInfo, value: string) => {
          drafts.set(keyOf(release, info.field), value);
        }}
        onToggleReset={(info: FieldInfo) => {
          toggleReset(keyOf(release, info.field));
        }}
      />
    {/if}

    <h3 class="group-title">Tracks</h3>
    <table>
      <thead>
        <tr>
          {#if discs || more}<th class="number">Disc</th>{/if}
          <th class="number">#</th>
          <th>Title</th>
          <th>Artist</th>
          {#if more}<th class="toggle"
              ><span class="visually-hidden">Track fields</span></th
            >{/if}
        </tr>
      </thead>
      <tbody>
        {#each data.tracks as row (row.id)}
          {@const target = track(row.id)}
          <tr>
            {#each COLUMNS.filter((c) => c !== 'disc' || discs || more) as column (column)}
              {@const info = field(row.fields, column)}
              <td
                class={column === 'disc' || column === 'track' ? 'number' : ''}
              >
                {#if info}
                  <FieldInput
                    {info}
                    name={`${info.label}, ${row.fileName}`}
                    value={current(target, info)}
                    reset={resets.has(keyOf(target, info.field))}
                    onInput={(value: string) => {
                      drafts.set(keyOf(target, info.field), value);
                    }}
                    onToggleReset={() => {
                      toggleReset(keyOf(target, info.field));
                    }}
                  />
                {/if}
              </td>
            {/each}
            {#if more}
              <td class="toggle">
                <button
                  type="button"
                  class="icon"
                  aria-expanded={expanded === row.id}
                  aria-label={`Fields of ${row.fileName}`}
                  onclick={() => {
                    expanded = expanded === row.id ? null : row.id;
                  }}><Icon name="chevron-down" size={16} /></button
                >
              </td>
            {/if}
          </tr>
          {#if more && expanded === row.id}
            <tr class="track-fields">
              <td colspan="5">
                <p class="muted truncate">{row.fileName}</p>
                <FieldGrid
                  fields={row.fields.filter((f) => !COLUMNS.includes(f.field))}
                  of={row.fileName}
                  valueOf={(info: FieldInfo) => current(target, info)}
                  isReset={(info: FieldInfo) =>
                    resets.has(keyOf(target, info.field))}
                  onInput={(info: FieldInfo, value: string) => {
                    drafts.set(keyOf(target, info.field), value);
                  }}
                  onToggleReset={(info: FieldInfo) => {
                    toggleReset(keyOf(target, info.field));
                  }}
                />
              </td>
            </tr>
          {/if}
        {/each}
      </tbody>
    </table>
  {/if}

  <div class="bar">
    <span class="problem" role="status">
      {#if leaving}
        Discard your changes?
        <button type="button" class="small danger" onclick={onBack}
          >Discard</button
        >
        <button
          type="button"
          class="small tertiary"
          onclick={() => (leaving = false)}>Keep editing</button
        >
      {:else}
        {problem ?? ''}
      {/if}
    </span>
    <button type="button" onclick={back}>Cancel</button>
    <button
      type="button"
      class="primary"
      disabled={saving || changes.length === 0}
      onclick={() => void save()}>Save</button
    >
  </div>
</section>

<style>
  .editor {
    max-width: 1100px;
  }
  header {
    display: flex;
    align-items: flex-end;
    gap: var(--space-5);
    margin: var(--space-3) 0 var(--space-5);
  }
  header .art {
    width: 96px;
    height: 96px;
  }
  .heading-text {
    display: grid;
    gap: var(--space-2);
  }
  .field {
    display: grid;
    gap: var(--space-1);
  }
  .genre {
    max-width: 480px;
    margin-top: var(--space-3);
  }
  .label {
    color: var(--text-secondary);
    font-size: var(--type-caption);
  }
  .more-toggle {
    margin-top: var(--space-3);
  }
  .hint {
    margin: var(--space-1) 0 var(--space-3);
    font-size: var(--type-small);
  }
  table {
    width: 100%;
    border-collapse: collapse;
  }
  th {
    text-align: left;
    color: var(--text-secondary);
    font-size: var(--type-caption);
    font-weight: var(--weight-regular);
    padding: var(--space-1);
  }
  td {
    padding: var(--space-1);
  }
  .number {
    width: 5rem;
  }
  .toggle {
    width: var(--hit);
  }
  .track-fields td {
    padding: var(--space-2) var(--space-1) var(--space-4);
  }
  /* Pinned to the window's bottom edge, over the view's bottom padding,
     so no row shows below it. */
  .bar {
    position: sticky;
    bottom: calc(-1 * var(--space-6));
    z-index: 1;
    display: flex;
    align-items: center;
    gap: var(--space-3);
    margin: var(--space-4) 0 calc(-1 * var(--space-6));
    padding: var(--space-3) 0 var(--space-6);
    border-top: 1px solid var(--line);
    background: var(--bg);
  }
  .problem {
    flex: 1;
    display: flex;
    align-items: center;
    gap: var(--space-2);
    color: var(--warning);
    font-size: var(--type-small);
  }
</style>
