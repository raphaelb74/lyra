import { describe, expect, it } from 'vitest';

import {
  NO_ALBUM_FILTER,
  filterAlbums,
  filterArtists,
  genresOf,
  sortTracks,
} from './filters';
import type { Album, Artist, LibraryTrack } from './ipc';

function album(
  title: string,
  artist: string,
  year: number | null,
  format: Partial<Album> = {},
): Album {
  return {
    id: title.length,
    title,
    artist,
    year,
    art: null,
    thumb: null,
    trackCount: 10,
    genre: null,
    durationSeconds: null,
    codec: 'flac',
    sampleRate: 44100,
    dsdMultiple: null,
    ...format,
  };
}

const albums = [
  album('Heroes', 'David Bowie', 1977, {
    codec: 'dsf',
    sampleRate: 2822400,
    dsdMultiple: 64,
    genre: 'Rock',
  }),
  album('Rumours', 'Fleetwood Mac', 1977, { sampleRate: 96000, genre: 'Rock' }),
  album('Abbey Road', 'The Beatles', 1969, { genre: 'Pop' }),
  album('Low', 'David Bowie', 1977),
];

const titles = (list: Album[]) => list.map((a) => a.title);

describe('album filter', () => {
  it('matches every word in title, artist or year', () => {
    const filter = { ...NO_ALBUM_FILTER, query: 'bowie 1977' };

    expect(titles(filterAlbums(albums, filter))).toEqual(['Heroes', 'Low']);
  });

  it('keeps a quality: Hi-Res includes DSD, CD is the rest', () => {
    const only = (quality: 'hires' | 'dsd' | 'cd') =>
      titles(filterAlbums(albums, { ...NO_ALBUM_FILTER, quality }));

    expect(only('hires')).toEqual(['Heroes', 'Rumours']);
    expect(only('dsd')).toEqual(['Heroes']);
    expect(only('cd')).toEqual(['Low', 'Abbey Road']);
  });

  it('keeps a genre and sorts', () => {
    const rock = { ...NO_ALBUM_FILTER, genre: 'Rock', sort: 'title' as const };

    expect(titles(filterAlbums(albums, rock))).toEqual(['Heroes', 'Rumours']);
    expect(
      titles(filterAlbums(albums, { ...NO_ALBUM_FILTER, sort: 'oldest' })),
    ).toEqual(['Abbey Road', 'Heroes', 'Low', 'Rumours']);
    expect(genresOf(albums)).toEqual(['Rock', 'Pop']);
  });
});

describe('artist filter', () => {
  const artist = (name: string, albumCount: number): Artist => ({
    id: albumCount,
    name,
    albumCount,
    trackCount: albumCount * 10,
    art: null,
  });

  const artists = [artist('Björk', 3), artist('ABBA', 1), artist('bauhaus', 2)];

  it('matches names and sorts by name or albums', () => {
    const names = (list: Artist[]) => list.map((a) => a.name);

    expect(names(filterArtists(artists, 'b', 'name'))).toEqual([
      'ABBA',
      'bauhaus',
      'Björk',
    ]);
    expect(names(filterArtists(artists, '', 'albums'))[0]).toBe('Björk');
  });
});

describe('track sort', () => {
  const track = (
    title: string,
    seconds: number,
    format: Partial<LibraryTrack>,
  ): LibraryTrack => ({
    id: seconds,
    title,
    artist: 'A',
    album: 'B',
    albumId: 1,
    disc: null,
    track: null,
    durationSeconds: seconds,
    codec: 'flac',
    sampleRate: 44100,
    bitsPerSample: 16,
    channels: 2,
    dsdMultiple: null,
    ...format,
  });

  const tracks = [
    track('b', 200, { sampleRate: 96000, bitsPerSample: 24 }),
    track('a', 300, {}),
    track('c', 100, { codec: 'dsf', bitsPerSample: 1, dsdMultiple: 64 }),
  ];

  const order = (column: 'title' | 'format' | 'length', descending = false) =>
    sortTracks(tracks, { column, descending }).map((t) => t.title);

  it('sorts by a column either way', () => {
    expect(order('title')).toEqual(['a', 'b', 'c']);
    expect(order('length', true)).toEqual(['a', 'b', 'c']);
    expect(order('format', true)).toEqual(['c', 'b', 'a']);
  });
});
