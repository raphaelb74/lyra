//! Play order: repeat, shuffle and queue edits.

use std::hash::{BuildHasher, RandomState};
use std::sync::Arc;

use crate::player::QueueItem;

/// What plays after a track ends.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Repeat {
    /// Stop after the last entry.
    #[default]
    Off,
    /// Start the queue again after the last entry.
    All,
    /// Play the same track again.
    One,
}

/// A change to the queue while it plays.
#[derive(Debug)]
pub(crate) enum Edit {
    /// Adds tracks after the playing one (`next`) or at the end.
    Add {
        items: Vec<QueueItem>,
        next: bool,
    },
    /// Moves entry `index` to play next.
    MoveNext(usize),
    Remove(usize),
    /// Removes every entry but the playing one.
    Clear,
}

/// The queue in play order.
#[derive(Clone, Debug)]
pub(crate) struct Queue {
    pub items: Arc<[QueueItem]>,
    /// Each entry's place in the order the listener gave, which shuffle off
    /// returns to. Equal ranks keep their play order. Equals the position
    /// while not shuffled.
    rank: Arc<[u64]>,
}

/// One entry while the queue is rebuilt: where it was, if it was.
struct Slot {
    from: Option<usize>,
    item: QueueItem,
    rank: u64,
}

/// For each old position, where the entry went (`None`: removed).
pub(crate) type Moved = Vec<Option<usize>>;

impl Queue {
    pub fn new(items: Vec<QueueItem>) -> Self {
        Self::from_items(Arc::from(items))
    }

    /// A queue in the given order; shares `items`.
    pub fn from_items(items: Arc<[QueueItem]>) -> Self {
        let rank = (0..items.len()).map(as_rank).collect();
        Self { items, rank }
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// The queue shuffled, with the entries at `front` (positions, in this
    /// order) kept first.
    pub fn shuffled(&self, front: &[usize]) -> (Self, Moved) {
        let mut rest: Vec<usize> = (0..self.len()).filter(|p| !front.contains(p)).collect();
        shuffle(&mut rest);
        let order = front.iter().chain(&rest).copied();
        Self::from_slots(order.map(|p| self.slot(p)).collect(), self.len(), false)
    }

    /// The queue back in the order it was given.
    pub fn unshuffled(&self) -> (Self, Moved) {
        let mut slots = self.slots();
        slots.sort_by_key(|slot| slot.rank); // stable: ties keep play order
        Self::from_slots(slots, self.len(), true)
    }

    /// The queue after `edit`. `committed` are the positions already playing
    /// or decoded, in play order: they stay, in that order, and new or
    /// promoted tracks go after them. `None` when the edit would touch them
    /// or names no entry.
    pub fn edited(&self, edit: Edit, committed: &[usize], shuffled: bool) -> Option<(Self, Moved)> {
        let mut slots = self.slots();
        // Where "next" is: after the last committed entry.
        let next = committed.last().map_or(0, |&last| last + 1);
        let movable = |index: usize| index < self.len() && !committed.contains(&index);
        match edit {
            Edit::Add { items, next: true } => {
                // Given order: beside the playing entry.
                let rank = next.checked_sub(1).map_or(0, |p| self.rank[p]);
                let added = items.into_iter().map(|item| Slot {
                    from: None,
                    item,
                    rank,
                });
                slots.splice(next..next, added);
            }
            Edit::Add { items, next: false } => {
                let rank = self.rank.iter().max().map_or(0, |r| r + 1);
                slots.extend(items.into_iter().map(|item| Slot {
                    from: None,
                    item,
                    rank,
                }));
            }
            Edit::MoveNext(index) => {
                if !movable(index) {
                    return None;
                }
                let slot = slots.remove(index);
                let at = if index < next { next - 1 } else { next };
                slots.insert(at, slot);
            }
            Edit::Remove(index) => {
                if !movable(index) {
                    return None;
                }
                slots.remove(index);
            }
            Edit::Clear => slots.retain(|slot| slot.from.is_some_and(|p| committed.contains(&p))),
        }
        Some(Self::from_slots(slots, self.len(), !shuffled))
    }

    fn slot(&self, position: usize) -> Slot {
        Slot {
            from: Some(position),
            item: self.items[position].clone(),
            rank: self.rank[position],
        }
    }

    fn slots(&self) -> Vec<Slot> {
        (0..self.len()).map(|p| self.slot(p)).collect()
    }

    /// `ranked`: the new play order is the given order, so ranks restart
    /// from the positions.
    fn from_slots(slots: Vec<Slot>, old_length: usize, ranked: bool) -> (Self, Moved) {
        let mut moved = vec![None; old_length];
        let mut items = Vec::with_capacity(slots.len());
        let mut rank = Vec::with_capacity(slots.len());
        for (new, slot) in slots.into_iter().enumerate() {
            if let Some(entry) = slot.from.and_then(|old| moved.get_mut(old)) {
                *entry = Some(new);
            }
            items.push(slot.item);
            rank.push(if ranked { as_rank(new) } else { slot.rank });
        }
        let queue = Self {
            items: Arc::from(items),
            rank: Arc::from(rank),
        };
        (queue, moved)
    }
}

fn as_rank(position: usize) -> u64 {
    u64::try_from(position).unwrap_or(u64::MAX)
}

/// The position after `index` in a queue of `length`. `skip` is the listener
/// asking for the next track, which moves on even when repeating one.
pub(crate) fn following(index: usize, length: usize, repeat: Repeat, skip: bool) -> Option<usize> {
    match repeat {
        Repeat::One if !skip => Some(index),
        Repeat::Off => (index + 1 < length).then_some(index + 1),
        Repeat::All | Repeat::One => (length > 0).then(|| (index + 1) % length),
    }
}

/// Fisher–Yates with keys from std's random hasher (seeded per process,
/// different for every `RandomState`); no crate needed for a play order.
fn shuffle<T>(items: &mut [T]) {
    let keys = RandomState::new();
    for i in (1..items.len()).rev() {
        let bound = u64::try_from(i + 1).unwrap_or(u64::MAX);
        let j = usize::try_from(keys.hash_one(i) % bound).unwrap_or(0);
        items.swap(i, j);
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{Edit, Queue, Repeat, following};
    use crate::player::QueueItem;

    fn items(ids: std::ops::Range<i64>) -> Vec<QueueItem> {
        ids.map(|id| QueueItem {
            path: PathBuf::new(),
            id: Some(id),
        })
        .collect()
    }

    fn ids(queue: &Queue) -> Vec<i64> {
        queue.items.iter().filter_map(|item| item.id).collect()
    }

    #[test]
    fn repeat_decides_what_follows() {
        assert_eq!(following(1, 3, Repeat::Off, false), Some(2));
        assert_eq!(following(2, 3, Repeat::Off, false), None);
        assert_eq!(following(2, 3, Repeat::All, false), Some(0));
        assert_eq!(following(1, 3, Repeat::One, false), Some(1));
        // Next while repeating one moves on, and wraps.
        assert_eq!(following(1, 3, Repeat::One, true), Some(2));
        assert_eq!(following(2, 3, Repeat::One, true), Some(0));
    }

    #[test]
    fn shuffle_keeps_the_playing_entries_first_and_unshuffle_restores() {
        let given = Queue::new(items(0..50));
        let (shuffled, moved) = given.shuffled(&[7, 8]);
        assert_eq!(&ids(&shuffled)[..2], [7, 8]);
        let mut sorted = ids(&shuffled);
        sorted.sort_unstable();
        assert_eq!(sorted, ids(&given), "every entry exactly once");
        assert_ne!(ids(&shuffled), ids(&given));
        for (old, new) in moved.iter().enumerate() {
            assert_eq!(new.map(|new| &shuffled.items[new]), Some(&given.items[old]));
        }

        let (restored, moved) = shuffled.unshuffled();
        assert_eq!(ids(&restored), ids(&given));
        assert_eq!(
            moved[0],
            Some(7),
            "the playing entry goes back to its place"
        );
    }

    #[test]
    fn reshuffling_a_shuffled_queue_still_restores_the_given_order() {
        let given = Queue::new(items(0..20));
        let (once, _) = given.shuffled(&[3]);
        let (twice, _) = once.shuffled(&[0]);
        assert_eq!(ids(&twice.unshuffled().0), ids(&given));
    }

    #[test]
    fn added_tracks_go_after_what_is_playing_or_at_the_end() {
        let queue = Queue::new(items(0..4));
        // Entry 1 plays, entry 2 is already decoded after it.
        let (next, moved) = queue
            .edited(
                Edit::Add {
                    items: items(10..12),
                    next: true,
                },
                &[1, 2],
                false,
            )
            .unwrap();
        assert_eq!(ids(&next), [0, 1, 2, 10, 11, 3]);
        assert_eq!(moved, [Some(0), Some(1), Some(2), Some(5)]);
        let (end, _) = queue
            .edited(
                Edit::Add {
                    items: items(10..11),
                    next: false,
                },
                &[1],
                false,
            )
            .unwrap();
        assert_eq!(ids(&end), [0, 1, 2, 3, 10]);
    }

    #[test]
    fn move_next_remove_and_clear_leave_the_playing_track_alone() {
        let queue = Queue::new(items(0..5));
        let promoted = |index| queue.edited(Edit::MoveNext(index), &[1], false);
        assert_eq!(ids(&promoted(4).unwrap().0), [0, 1, 4, 2, 3]);
        assert_eq!(ids(&promoted(0).unwrap().0), [1, 0, 2, 3, 4]);
        assert!(promoted(1).is_none(), "the playing track stays");
        assert!(promoted(9).is_none());

        let (removed, moved) = queue.edited(Edit::Remove(3), &[1], false).unwrap();
        assert_eq!(ids(&removed), [0, 1, 2, 4]);
        assert_eq!(moved[3], None);
        assert!(queue.edited(Edit::Remove(1), &[1], false).is_none());

        let (cleared, moved) = queue.edited(Edit::Clear, &[2], false).unwrap();
        assert_eq!(ids(&cleared), [2]);
        assert_eq!(moved[2], Some(0));
        let (empty, _) = queue.edited(Edit::Clear, &[], false).unwrap();
        assert_eq!(empty.len(), 0);
    }

    #[test]
    fn edits_while_shuffled_still_unshuffle_to_the_given_order() {
        let given = Queue::new(items(0..6));
        let (shuffled, _) = given.shuffled(&[2]);
        let (added, _) = shuffled
            .edited(
                Edit::Add {
                    items: items(10..11),
                    next: true,
                },
                &[0],
                true,
            )
            .unwrap();
        assert_eq!(ids(&added)[..2], [2, 10]);
        // The added track stays after the one it was added beside.
        assert_eq!(ids(&added.unshuffled().0), [0, 1, 2, 10, 3, 4, 5]);
    }
}
