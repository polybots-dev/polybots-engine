//! Short lists stored in place, so a tile, unit or player owns no heap data and copying a game
//! copies flat arrays.
//!
//! `Inline<T, N>` keeps up to `N` values in the order they were added, which is the order the
//! checkpoint lists them in, and reads like a slice. Its capacity is a bound of the game (seats,
//! effect kinds, techs), not a tuning value: going past it is a broken invariant and panics
//! rather than dropping an entry, and where a command that did not come from the legal list
//! could get there, the rule uses `try_push` and refuses instead. `Counts<N>` keeps how many times each id below `N` was added,
//! for lists that can grow without bound and whose order nothing reads.

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Clone, Copy)]
pub struct Inline<T, const N: usize> {
    len: u8,
    items: [T; N],
}

impl<T: Copy + Default, const N: usize> Default for Inline<T, N> {
    fn default() -> Self {
        Inline {
            len: 0,
            items: [T::default(); N],
        }
    }
}

impl<T: Copy + Default, const N: usize> Inline<T, N> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add `v`, or say why not: for lists a command from outside the legal list could overfill.
    pub fn try_push(&mut self, v: T) -> Result<(), String> {
        let slot = self
            .items
            .get_mut(usize::from(self.len))
            .ok_or_else(|| format!("short list of {N} is full"))?;
        *slot = v;
        self.len += 1;
        Ok(())
    }

    pub fn push(&mut self, v: T) {
        if let Err(e) = self.try_push(v) {
            panic!("{e}");
        }
    }

    pub fn clear(&mut self) {
        self.len = 0;
    }

    /// Keep the entries `keep` accepts, in order.
    pub fn retain(&mut self, mut keep: impl FnMut(&T) -> bool) {
        let mut kept = 0;
        for i in 0..usize::from(self.len) {
            if keep(&self.items[i]) {
                self.items[kept] = self.items[i];
                kept += 1;
            }
        }
        self.len = kept as u8;
    }

    /// Remove and return the entry at `i`, keeping the order of the rest.
    pub fn remove(&mut self, i: usize) -> T {
        let n = usize::from(self.len);
        assert!(i < n, "short list index out of range");
        let v = self.items[i];
        self.items.copy_within(i + 1..n, i);
        self.len -= 1;
        v
    }
}

impl<T, const N: usize> std::ops::Deref for Inline<T, N> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        &self.items[..usize::from(self.len)]
    }
}

impl<T, const N: usize> std::ops::DerefMut for Inline<T, N> {
    fn deref_mut(&mut self) -> &mut [T] {
        &mut self.items[..usize::from(self.len)]
    }
}

impl<T: PartialEq, const N: usize> PartialEq for Inline<T, N> {
    fn eq(&self, other: &Self) -> bool {
        **self == **other
    }
}

impl<T: std::fmt::Debug, const N: usize> std::fmt::Debug for Inline<T, N> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

impl<T: Copy + Default, const N: usize> FromIterator<T> for Inline<T, N> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        let mut out = Self::default();
        for v in iter {
            out.push(v);
        }
        out
    }
}

impl<'a, T, const N: usize> IntoIterator for &'a Inline<T, N> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<T: Serialize, const N: usize> Serialize for Inline<T, N> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_seq(self.iter())
    }
}

impl<'de, T: Copy + Default + Deserialize<'de>, const N: usize> Deserialize<'de> for Inline<T, N> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let mut out = Self::default();
        for v in Vec::<T>::deserialize(d)? {
            out.try_push(v).map_err(D::Error::custom)?;
        }
        Ok(out)
    }
}

/// A list with at most one entry per player: up to four seats and Nature.
pub type Seats<T> = Inline<T, 5>;

/// How many times each id below `N` was added. Written as a list sorted by id.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Counts<const N: usize>([u8; N]);

impl<const N: usize> Default for Counts<N> {
    fn default() -> Self {
        Counts([0; N])
    }
}

impl<const N: usize> Counts<N> {
    pub fn try_push(&mut self, v: i32) -> Result<(), String> {
        let slot = usize::try_from(v)
            .ok()
            .and_then(|i| self.0.get_mut(i))
            .ok_or_else(|| format!("id {v} is outside the {N} counted"))?;
        *slot = slot
            .checked_add(1)
            .ok_or_else(|| format!("more than 255 of id {v}"))?;
        Ok(())
    }

    pub fn push(&mut self, v: i32) {
        if let Err(e) = self.try_push(v) {
            panic!("{e}");
        }
    }

    pub fn count(&self, v: i32) -> usize {
        usize::try_from(v)
            .ok()
            .and_then(|i| self.0.get(i))
            .map_or(0, |&n| usize::from(n))
    }

    pub fn contains(&self, v: &i32) -> bool {
        self.count(*v) > 0
    }

    pub fn len(&self) -> usize {
        self.0.iter().map(|&n| usize::from(n)).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.0.iter().all(|&n| n == 0)
    }

    /// Every id, repeated as often as it was added, in id order.
    pub fn iter(&self) -> impl Iterator<Item = i32> + '_ {
        self.0
            .iter()
            .enumerate()
            .flat_map(|(id, &n)| std::iter::repeat_n(id as i32, usize::from(n)))
    }
}

impl<const N: usize> std::fmt::Debug for Counts<N> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

impl<const N: usize> Serialize for Counts<N> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_seq(self.iter())
    }
}

impl<'de, const N: usize> Deserialize<'de> for Counts<N> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let mut out = Self::default();
        for v in Vec::<i32>::deserialize(d)? {
            out.try_push(v).map_err(D::Error::custom)?;
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_keeps_order_and_reads_back() {
        let mut l: Inline<i32, 4> = [3, 255, 1].into_iter().collect();
        assert_eq!(*l, [3, 255, 1]);
        assert!(l.contains(&255) && !l.contains(&2));
        l.retain(|&v| v != 255);
        l.push(7);
        assert_eq!(l.remove(0), 3);
        assert_eq!(serde_json::to_string(&l).unwrap(), "[1,7]");
        let back: Inline<i32, 4> = serde_json::from_str("[1,7]").unwrap();
        assert_eq!(back, l);
    }

    #[test]
    fn inline_refuses_more_than_it_holds() {
        assert!(serde_json::from_str::<Inline<i32, 2>>("[1,2,3]").is_err());
    }

    #[test]
    #[should_panic(expected = "full")]
    fn inline_push_past_capacity_panics() {
        let mut l: Inline<i32, 1> = Inline::new();
        l.push(1);
        l.push(2);
    }

    #[test]
    fn counts_keep_repeats_and_write_them_sorted() {
        let c: Counts<14> = serde_json::from_str("[8,2,8,1]").unwrap();
        assert_eq!((c.count(8), c.count(2), c.count(3), c.len()), (2, 1, 0, 4));
        assert_eq!(serde_json::to_string(&c).unwrap(), "[1,2,8,8]");
        assert!(serde_json::from_str::<Counts<14>>("[14]").is_err());
    }
}
