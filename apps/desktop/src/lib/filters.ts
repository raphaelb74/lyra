/** Filtering and sorting of library lists, done in the view (data is loaded whole). */

import type { Album, Artist, LibraryTrack, SourceFormat } from './ipc';

/** Compares names as people read them: case and accents aside, numbers by value. */
const collator = new Intl.Collator(undefined, {
  sensitivity: 'base',
  numeric: true,
});

/** The words of a filter query, lower case. */
export function words(query: string): string[] {
  return query.toLowerCase().split(/\s+/).filter(Boolean);
}

/** Every word appears somewhere in `text` (already lower case). */
export function matchesAll(text: string, wanted: readonly string[]): boolean {
  return wanted.every((word) => text.includes(word));
}

/** One titled set of choices in a filter menu, one of them chosen. */
export interface FilterGroup {
  title: string;
  options: readonly { value: string; label: string }[];
  value: string;
  onChoose: (value: string) => void;
}

/** The option of `options` whose value is `value`, typed. */
export function chosen<T extends string>(
  options: readonly { value: T }[],
  value: string,
): T | undefined {
  return options.find((option) => option.value === value)?.value;
}

export const QUALITIES: readonly { value: Quality; label: string }[] = [
  { value: 'all', label: 'Any quality' },
  { value: 'hires', label: 'Hi-Res (incl. DSD)' },
  { value: 'dsd', label: 'DSD only' },
  { value: 'cd', label: 'CD quality' },
];

export const ALBUM_SORTS: readonly { value: AlbumSort; label: string }[] = [
  { value: 'artist', label: 'Artist' },
  { value: 'title', label: 'Title' },
  { value: 'newest', label: 'Newest first' },
  { value: 'oldest', label: 'Oldest first' },
];

export const ARTIST_SORTS: readonly { value: ArtistSort; label: string }[] = [
  { value: 'name', label: 'Name' },
  { value: 'albums', label: 'Most albums' },
  { value: 'tracks', label: 'Most tracks' },
];

export type Quality = 'all' | 'hires' | 'dsd' | 'cd';

/** What an album's best format is: DSD, hi-res PCM (above 48 kHz) or CD. */
export function qualityOf(format: SourceFormat): Exclude<Quality, 'all'> {
  if (format.dsdMultiple !== null) return 'dsd';

  return format.sampleRate > 48000 ? 'hires' : 'cd';
}

export type AlbumSort = 'artist' | 'title' | 'newest' | 'oldest';

export interface AlbumFilter {
  query: string;
  /** Hi-Res includes DSD. */
  quality: Quality;
  genre: string | null;
  sort: AlbumSort;
}

export const NO_ALBUM_FILTER: AlbumFilter = {
  query: '',
  quality: 'all',
  genre: null,
  sort: 'artist',
};

/** Albums matching `filter`, in its order. */
export function filterAlbums(
  albums: readonly Album[],
  filter: AlbumFilter,
): Album[] {
  const wanted = words(filter.query);

  const kept = albums.filter((album) => {
    const quality = qualityOf(album);

    if (filter.quality === 'hires' && quality === 'cd') return false;

    if (filter.quality !== 'all' && filter.quality !== 'hires') {
      if (quality !== filter.quality) return false;
    }

    if (filter.genre !== null && album.genre !== filter.genre) return false;

    const text = `${album.title} ${album.artist} ${String(album.year ?? '')}`;

    return matchesAll(text.toLowerCase(), wanted);
  });

  return kept.sort(albumOrder(filter.sort));
}

function albumOrder(sort: AlbumSort): (a: Album, b: Album) => number {
  const year = (album: Album) => album.year ?? 0;

  switch (sort) {
    case 'title':
      return (a, b) => collator.compare(a.title, b.title);
    case 'newest':
      return (a, b) => year(b) - year(a) || collator.compare(a.title, b.title);
    case 'oldest':
      return (a, b) => year(a) - year(b) || collator.compare(a.title, b.title);
    case 'artist':
      return (a, b) =>
        collator.compare(a.artist, b.artist) ||
        year(a) - year(b) ||
        collator.compare(a.title, b.title);
  }
}

/** The genres among `albums`, most albums first. */
export function genresOf(albums: readonly Album[]): string[] {
  const counts = new Map<string, number>();

  for (const album of albums) {
    if (album.genre !== null) {
      counts.set(album.genre, (counts.get(album.genre) ?? 0) + 1);
    }
  }

  return [...counts.entries()]
    .sort((a, b) => b[1] - a[1] || collator.compare(a[0], b[0]))
    .map(([genre]) => genre);
}

export type ArtistSort = 'name' | 'albums' | 'tracks';

/** Artists whose name matches `query`, in `sort` order. */
export function filterArtists(
  artists: readonly Artist[],
  query: string,
  sort: ArtistSort,
): Artist[] {
  const wanted = words(query);

  const kept = artists.filter((artist) =>
    matchesAll(artist.name.toLowerCase(), wanted),
  );

  const byName = (a: Artist, b: Artist) => collator.compare(a.name, b.name);

  switch (sort) {
    case 'albums':
      return kept.sort((a, b) => b.albumCount - a.albumCount || byName(a, b));
    case 'tracks':
      return kept.sort((a, b) => b.trackCount - a.trackCount || byName(a, b));
    case 'name':
      return kept.sort(byName);
  }
}

export type TrackColumn = 'title' | 'artist' | 'album' | 'format' | 'length';

export interface TrackSort {
  column: TrackColumn;
  descending: boolean;
}

/** Highest resolution first when descending; DSD above any PCM. */
function resolution(track: LibraryTrack): number {
  return track.dsdMultiple === null
    ? track.sampleRate * track.bitsPerSample
    : Number.MAX_SAFE_INTEGER / 1024 + track.dsdMultiple;
}

/** `tracks` sorted by one column; ties keep the library order. */
export function sortTracks(
  tracks: readonly LibraryTrack[],
  sort: TrackSort,
): LibraryTrack[] {
  const compare = (a: LibraryTrack, b: LibraryTrack): number => {
    switch (sort.column) {
      case 'title':
        return collator.compare(a.title, b.title);
      case 'artist':
        return collator.compare(a.artist, b.artist);
      case 'album':
        return collator.compare(a.album, b.album);
      case 'format':
        return resolution(a) - resolution(b);
      case 'length':
        return (a.durationSeconds ?? 0) - (b.durationSeconds ?? 0);
    }
  };

  const sign = sort.descending ? -1 : 1;

  return [...tracks].sort((a, b) => sign * compare(a, b));
}
