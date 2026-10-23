<script lang="ts">
  import { untrack } from 'svelte';
  import { SvelteSet } from 'svelte/reactivity';

  import Icon from './Icon.svelte';
  import { clock } from './format';
  import {
    BackendError,
    type Album,
    type Backend,
    type Candidate,
    type Evidence,
    type MatchInfo,
    type MatchTrack,
    type PairChoice,
    type PairedBy,
    type Slot,
    type Suggestion,
  } from './ipc';

  interface Props {
    api: Backend;
    album: Album;
    onBack: () => void;
    /** After a save, which the album page can undo. */
    onSaved: () => void;
  }

  const { api, album, onBack, onSaved }: Props = $props();

  /** Why the last request failed, with the error's code. */
  let problem = $state<{ message: string; code: string } | null>(null);
  let candidates = $state<Candidate[] | null>(null);
  let searching = $state(false);
  const typed = $state(
    untrack(() => ({ title: album.title, artist: album.artist })),
  );

  /** The edition compared with the album. */
  let info = $state<MatchInfo | null>(null);
  /** The edition being fetched for comparison. */
  let comparing = $state<string | null>(null);
  /** Pairs as the listener set them; null while automatic. */
  let chosen = $state<PairChoice[] | null>(null);
  /** The edition's cover as a data URL; null when it has none, undefined
   * while it loads. */
  let cover = $state<string | null | undefined>(undefined);
  let useCover = $state(false);
  /** Changes unticked. */
  const skipped = new SvelteSet<string>();
  let saving = $state(false);

  /** Only the latest request's answer is shown. */
  let ticket = 0;

  const EVIDENCE: readonly [keyof Evidence, string][] = [
    ['releaseId', 'Tagged edition'],
    ['trackCount', 'Same track count'],
    ['title', 'Same title'],
    ['artist', 'Same artist'],
    ['year', 'Same year'],
  ];

  const PAIRED_BY: Record<PairedBy, string> = {
    id: 'MusicBrainz ID',
    number: 'Number',
    runningNumber: 'Number across discs',
    order: 'Order',
    hand: 'By you',
  };

  const tracksById = $derived(
    new Map((info?.tracks ?? []).map((track) => [track.id, track])),
  );
  const pairsBySlot = $derived(
    new Map((info?.pairs ?? []).map((pair) => [slotKey(pair.slot), pair])),
  );
  const accepted = $derived(
    (info?.suggestions ?? []).filter(
      (suggestion) => !suggestion.kept && !skipped.has(keyOf(suggestion)),
    ),
  );
  const kept = $derived(
    (info?.suggestions ?? []).filter((suggestion) => suggestion.kept).length,
  );

  function slotKey(slot: Slot): string {
    return `${String(slot.medium)}:${String(slot.index)}`;
  }

  function keyOf(suggestion: Suggestion): string {
    return `${suggestion.target.kind}:${String(suggestion.target.id)}:${suggestion.field}`;
  }

  /** Runs a request; its answer is dropped when a later one started. */
  async function ask<T>(request: () => Promise<T>): Promise<T | undefined> {
    ticket += 1;
    const mine = ticket;

    problem = null;

    try {
      const answer = await request();

      return mine === ticket ? answer : undefined;
    } catch (error) {
      if (mine === ticket) {
        problem =
          error instanceof BackendError
            ? { message: error.message, code: error.code }
            : { message: 'Something went wrong. Try again.', code: '' };
      }

      return undefined;
    }
  }

  async function find(title: string | null, artist: string | null) {
    searching = true;
    const found = await ask(() => api.matchCandidates(album.id, title, artist));

    searching = false;

    if (found !== undefined) candidates = found;
  }

  function search(event: SubmitEvent) {
    event.preventDefault();

    const title = typed.title.trim();

    if (title === '') void find(null, null);
    else void find(title, typed.artist.trim());
  }

  async function compare(candidate: Candidate) {
    comparing = candidate.releaseId;
    chosen = null;
    useCover = false;
    cover = undefined;
    skipped.clear();

    const found = await ask(() =>
      api.matchRelease(album.id, candidate.releaseId),
    );

    comparing = null;

    if (found === undefined) return;

    info = found;
    void loadCover(found.release.releaseId);
  }

  /** The cover arrives after the comparison; failing to get it is no error. */
  async function loadCover(releaseId: string) {
    let found: string | null;

    try {
      found = await api.matchCover(releaseId);
    } catch {
      found = null;
    }

    if (info?.release.releaseId === releaseId) cover = found;
  }

  /** Compares again with `pairs`, or automatic pairing when null. */
  async function repair(pairs: PairChoice[] | null) {
    const current = info;

    if (current === null) return;

    const found = await ask(() =>
      api.matchRelease(album.id, current.release.releaseId, pairs),
    );

    if (
      found === undefined ||
      info?.release.releaseId !== found.release.releaseId
    )
      return;

    info = found;
    chosen = pairs;
  }

  /** Puts `trackId` at `slot`, or leaves the slot empty when null. */
  function pairAt(slot: Slot, trackId: number | null) {
    const others = (info?.pairs ?? [])
      .filter(
        (pair) =>
          slotKey(pair.slot) !== slotKey(slot) && pair.track !== trackId,
      )
      .map((pair) => ({ track: pair.track, slot: pair.slot }));

    void repair(
      trackId === null ? others : [...others, { track: trackId, slot }],
    );
  }

  async function save() {
    const current = info;

    if (current === null) return;

    saving = true;

    const done = await ask(() =>
      api.applyMatch(
        album.id,
        current.release.releaseId,
        chosen,
        accepted.map((suggestion) => ({
          target: suggestion.target,
          field: suggestion.field,
        })),
        current.revisions,
        useCover,
      ),
    );

    saving = false;

    if (done !== undefined) {
      onSaved();

      return;
    }

    const refused = problem;

    if (refused?.code === 'changed_meanwhile') {
      // Show the album as it is now; ticks stay.
      await repair(chosen);
      problem = refused;
    }
  }

  function trackLabel(track: MatchTrack): string {
    const place = [track.disc, track.number].filter(Boolean).join('-');
    const title = track.title ?? 'Untitled';

    return place === '' ? title : `${place}. ${title}`;
  }

  function itemOf(suggestion: Suggestion): string {
    if (suggestion.target.kind === 'release') return 'Album';

    const track = tracksById.get(suggestion.target.id);

    return track ? trackLabel(track) : 'Track';
  }

  function shown(values: string[]): string {
    return values.length === 0 ? '(empty)' : values.join('; ');
  }

  function seconds(value: number | null): string {
    return value === null ? '' : clock(value);
  }

  /** `+0:12` or `−0:12`. */
  function difference(value: number): string {
    return `${value < 0 ? '−' : '+'}${clock(Math.abs(value))}`;
  }

  function details(candidate: Candidate): string {
    const tracks =
      candidate.trackCount === null
        ? null
        : `${String(candidate.trackCount)} tracks`;

    return [
      candidate.date,
      candidate.country,
      candidate.formats.join(' + '),
      tracks,
      candidate.labels.join(', '),
      candidate.status === 'Official' ? null : candidate.status,
    ]
      .filter(Boolean)
      .join(' · ');
  }

  function plural(count: number): string {
    return `${String(count)} ${count === 1 ? 'change' : 'changes'}`;
  }

  const saveLabel = $derived.by(() => {
    if (accepted.length === 0) return useCover ? 'Save cover' : 'Save';

    return `Save ${plural(accepted.length)}${useCover ? ' and cover' : ''}`;
  });

  $effect(() => {
    untrack(() => {
      void find(null, null);
    });
  });
</script>

<section class="match" aria-labelledby="match-title">
  {#if info}
    <button type="button" class="back" onclick={() => (info = null)}
      ><Icon name="chevron-left" size={16} />All editions</button
    >
  {:else}
    <button type="button" class="back" onclick={onBack}
      ><Icon name="chevron-left" size={16} />Back to album</button
    >
  {/if}
  <div class="title">
    <h2 id="match-title">ID Match</h2>
    <p class="muted">
      {album.title} · from MusicBrainz, saved in Lyra only
    </p>
  </div>

  {#if problem}
    <p class={problem.code === 'online_off' ? 'note' : 'alert'} role="alert">
      {problem.message}
    </p>
  {/if}

  {#if info}
    {@const release = info.release}
    <header class="edition">
      <div class="cover">
        {#if cover}
          <img class="art" src={cover} alt={`Cover of ${release.title}`} />
        {:else}
          <div class="art">
            <span class="muted"
              >{cover === undefined ? 'Loading cover…' : 'No cover'}</span
            >
          </div>
        {/if}
      </div>
      <div class="about">
        <h3>{release.title}</h3>
        <p>{release.artist}</p>
        {#if release.disambiguation}
          <p class="muted">{release.disambiguation}</p>
        {/if}
        <p class="muted">{details(release)}</p>
        <p class="muted id" title="MusicBrainz release ID">
          {release.releaseId}
        </p>
        {#if cover}
          <label class="use-cover">
            <input type="checkbox" bind:checked={useCover} />
            Use this cover
          </label>
          <p class="muted small">
            Undo keeps the cover; Edit → Use the files' cover brings yours back.
          </p>
        {/if}
      </div>
    </header>

    <div class="heading pairs-heading">
      <h3 class="group-title">Tracks</h3>
      {#if chosen !== null}
        <button
          type="button"
          class="tertiary small"
          onclick={() => void repair(null)}>Pair automatically</button
        >
      {/if}
    </div>
    <div class="panel">
      <table class="pairs">
        <thead>
          <tr>
            <th class="no">#</th>
            <th>On MusicBrainz</th>
            <th class="len">Length</th>
            <th>Your track</th>
            <th class="by">Paired by</th>
          </tr>
        </thead>
        {#each info.media as medium, m (m)}
          <tbody>
            {#if info.media.length > 1}
              <tr class="medium">
                <th colspan="5">
                  Disc {medium.position}{medium.format
                    ? ` · ${medium.format}`
                    : ''}{medium.title ? ` · ${medium.title}` : ''}
                </th>
              </tr>
            {/if}
            {#each medium.tracks as track, t (t)}
              {@const slot = { medium: m, index: t }}
              {@const pair = pairsBySlot.get(slotKey(slot))}
              {@const mine = pair ? tracksById.get(pair.track) : undefined}
              <tr>
                <td class="no muted">{track.number}</td>
                <td title={track.title}>{track.title}</td>
                <td class="len muted num">{seconds(track.seconds)}</td>
                <td>
                  <div class="yours">
                    <select
                      aria-label={`Your track for ${track.number}, ${track.title}`}
                      value={pair ? String(pair.track) : ''}
                      onchange={(event) => {
                        const value = event.currentTarget.value;

                        pairAt(slot, value === '' ? null : Number(value));
                      }}
                    >
                      <option value="">None</option>
                      {#each info.tracks as local (local.id)}
                        <option value={String(local.id)}
                          >{trackLabel(local)}</option
                        >
                      {/each}
                    </select>
                    {#if pair?.lengthsAgree === false && pair.lengthDifferenceSeconds !== null}
                      <span class="note num"
                        >{seconds(mine?.seconds ?? null)} ({difference(
                          pair.lengthDifferenceSeconds,
                        )})</span
                      >
                    {/if}
                  </div>
                </td>
                <td class="by muted">{pair ? PAIRED_BY[pair.by] : ''}</td>
              </tr>
            {/each}
          </tbody>
        {/each}
      </table>
    </div>
    {#if info.unpaired.length > 0}
      <p class="note unpaired">
        Not in this edition:
        {info.unpaired
          .map((id) => {
            const track = tracksById.get(id);

            return track ? trackLabel(track) : '';
          })
          .join(', ')}. Their tags stay as they are.
      </p>
    {/if}

    <h3 class="group-title">Changes</h3>
    {#if info.suggestions.length === 0}
      <p class="muted">Your tags already agree with this edition.</p>
    {:else}
      <div class="panel">
        <table class="changes">
          <thead>
            <tr>
              <th class="tick"><span class="visually-hidden">Save</span></th>
              <th>Item</th>
              <th class="what">Field</th>
              <th>Now</th>
              <th>MusicBrainz</th>
            </tr>
          </thead>
          <tbody>
            {#each info.suggestions as suggestion (keyOf(suggestion))}
              {@const item = itemOf(suggestion)}
              <tr class:kept={suggestion.kept}>
                <td class="tick">
                  <input
                    type="checkbox"
                    aria-label={`Save ${suggestion.fieldLabel} of ${item}`}
                    disabled={suggestion.kept}
                    checked={!suggestion.kept &&
                      !skipped.has(keyOf(suggestion))}
                    onchange={() => {
                      const key = keyOf(suggestion);

                      if (skipped.has(key)) skipped.delete(key);
                      else skipped.add(key);
                    }}
                  />
                </td>
                <td title={item}>{item}</td>
                <td class="muted">{suggestion.fieldLabel}</td>
                <td
                  class="muted"
                  title={suggestion.kept
                    ? 'You edited this field; your edit is kept.'
                    : shown(suggestion.before)}
                  >{shown(suggestion.before)}{#if suggestion.kept}
                    · yours, kept{/if}</td
                >
                <td title={shown(suggestion.after)}
                  >{shown(suggestion.after)}</td
                >
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
      {#if kept > 0}
        <p class="muted small">
          Fields you edited yourself keep your edit. To use MusicBrainz's value,
          reset them in Edit first.
        </p>
      {/if}
    {/if}

    <p class="muted small credit">
      Data from MusicBrainz (CC0). Covers from the Cover Art Archive belong to
      their owners.
    </p>
    <div class="bar">
      <button
        type="button"
        class="primary"
        disabled={saving || (accepted.length === 0 && !useCover)}
        onclick={() => void save()}>{saveLabel}</button
      >
    </div>
  {:else}
    <form class="search" onsubmit={search}>
      <input
        type="search"
        aria-label="Album title"
        placeholder="Album title"
        bind:value={typed.title}
      />
      <input
        type="search"
        aria-label="Artist"
        placeholder="Artist"
        bind:value={typed.artist}
      />
      <button type="submit" class="with-icon" disabled={searching}
        ><Icon name="search" size={16} />Search</button
      >
    </form>

    {#if candidates === null}
      {#if searching}
        <p class="muted" role="status">Asking MusicBrainz…</p>
      {/if}
    {:else if candidates.length === 0}
      <p class="muted" role="status">
        No edition found. Try a shorter title, or the artist alone.
      </p>
    {:else}
      <ul class="panel editions">
        {#each candidates as candidate (candidate.releaseId)}
          {@const agrees = EVIDENCE.filter(([key]) => candidate.evidence[key])}
          <li class="setting">
            <div class="setting-text">
              <h4 class="truncate">
                {candidate.title}
                <span class="muted">· {candidate.artist}</span>
              </h4>
              {#if candidate.disambiguation}
                <p class="muted truncate">{candidate.disambiguation}</p>
              {/if}
              <p class="muted truncate">{details(candidate)}</p>
              {#if agrees.length > 0}
                <ul class="badges" aria-label="Agrees with your album">
                  {#each agrees as [key, label] (key)}
                    <li>{label}</li>
                  {/each}
                </ul>
              {/if}
            </div>
            <button
              type="button"
              class="small"
              disabled={comparing !== null}
              onclick={() => void compare(candidate)}
              >{comparing === candidate.releaseId
                ? 'Comparing…'
                : 'Compare'}</button
            >
          </li>
        {/each}
      </ul>
      <p class="muted small credit">
        Ranked by what agrees with your tags; check the edition before saving.
      </p>
    {/if}
  {/if}
</section>

<style>
  .match {
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
  .note[role='alert'] {
    margin-bottom: var(--space-4);
  }
  .small {
    font-size: var(--type-small);
  }
  .search {
    display: flex;
    gap: var(--space-2);
    margin-bottom: var(--space-4);
  }
  .search input {
    flex: 1;
    min-width: 0;
  }
  .editions {
    list-style: none;
    margin: 0;
  }
  .badges {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-1);
    margin: var(--space-1) 0 0;
    padding: 0;
    list-style: none;
  }
  .badges li {
    font-size: var(--type-caption);
    color: var(--accent);
    border: 1px solid var(--line);
    border-radius: var(--radius-s);
    padding: 0 var(--space-1);
  }
  .credit {
    margin-top: var(--space-3);
  }
  .edition {
    display: flex;
    gap: var(--space-5);
    align-items: flex-start;
  }
  .cover .art {
    width: 160px;
    font-size: var(--type-caption);
    text-align: center;
  }
  .about {
    display: grid;
    gap: var(--space-1);
    min-width: 0;
  }
  .about h3 {
    font-size: var(--type-title);
    font-weight: var(--weight-strong);
  }
  .id {
    font-size: var(--type-caption);
    user-select: text;
  }
  .use-cover {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin-top: var(--space-2);
  }
  .pairs-heading {
    align-items: baseline;
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
  .medium th {
    padding-top: var(--space-3);
  }
  .no {
    width: 3rem;
  }
  .len {
    width: 4.5rem;
  }
  .by {
    width: 9rem;
  }
  .yours {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .yours select {
    flex: 1;
    min-width: 0;
  }
  .unpaired {
    margin-top: var(--space-2);
  }
  .tick {
    width: 2rem;
  }
  .what {
    width: 9rem;
  }
  .kept td {
    color: var(--text-muted);
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
