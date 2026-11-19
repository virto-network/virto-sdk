//! Layered state: the snapshot as a base, one forward diff per local block, and
//! a pending diff while a block is being built. The state at block `n` stacks
//! the diffs of every unpruned block up to `n` on the base.

use std::collections::BTreeMap;
use std::ops::Bound;

use smoldot::trie::DEFAULT_CHILD_STORAGE_PREFIX;

/// The key-value entries of one trie (main or child).
pub type Entries = BTreeMap<Vec<u8>, Vec<u8>>;
/// Changes to one trie; `None` deletes.
pub type EntriesDiff = BTreeMap<Vec<u8>, Option<Vec<u8>>>;

/// Child tries are keyed by their unprefixed id; the runtime and the RPC
/// sometimes pass the `:child_storage:default:` key instead.
fn child_id(child: &[u8]) -> &[u8] {
    child
        .strip_prefix(DEFAULT_CHILD_STORAGE_PREFIX)
        .unwrap_or(child)
}

/// The full state of the fork block.
#[derive(Default, Clone)]
pub struct Base {
    pub main: Entries,
    /// Child tries by unprefixed id.
    pub child: BTreeMap<Vec<u8>, Entries>,
}

impl Base {
    pub fn apply(&mut self, diff: Diff) {
        fn apply(entries: &mut Entries, diff: EntriesDiff) {
            for (k, v) in diff {
                match v {
                    Some(v) => entries.insert(k, v),
                    None => entries.remove(&k),
                };
            }
        }
        apply(&mut self.main, diff.main);
        for (id, d) in diff.child {
            apply(self.child.entry(id).or_default(), d);
        }
    }

    fn entries(&self, child: Option<&[u8]>) -> Option<&Entries> {
        match child {
            None => Some(&self.main),
            Some(c) => self.child.get(child_id(c)),
        }
    }
}

/// Changes made by a block, a runtime call or a direct write.
#[derive(Default, Clone, Debug, PartialEq)]
pub struct Diff {
    pub main: EntriesDiff,
    pub child: BTreeMap<Vec<u8>, EntriesDiff>,
}

impl Diff {
    /// Set a main trie value.
    pub fn insert(&mut self, key: impl Into<Vec<u8>>, value: impl Into<Vec<u8>>) {
        self.main.insert(key.into(), Some(value.into()));
    }

    /// Delete a main trie value.
    pub fn remove(&mut self, key: impl Into<Vec<u8>>) {
        self.main.insert(key.into(), None);
    }

    /// Set (`Some`) or delete (`None`) a value of the main trie or a child trie.
    pub fn set(&mut self, child: Option<&[u8]>, key: &[u8], value: Option<&[u8]>) {
        let entries = match child {
            None => &mut self.main,
            Some(c) => self.child.entry(child_id(c).to_vec()).or_default(),
        };
        entries.insert(key.to_vec(), value.map(<[u8]>::to_vec));
    }

    pub fn merge(&mut self, other: Diff) {
        self.main.extend(other.main);
        for (c, d) in other.child {
            self.child.entry(c).or_default().extend(d);
        }
    }

    pub fn touches(&self, key: &[u8]) -> bool {
        self.main.contains_key(key)
    }

    fn entries(&self, child: Option<&[u8]>) -> Option<&EntriesDiff> {
        match child {
            None => Some(&self.main),
            Some(c) => self.child.get(child_id(c)),
        }
    }

    /// `Some(value_or_deletion)` if this diff touches `key`.
    fn get(&self, child: Option<&[u8]>, key: &[u8]) -> Option<Option<&[u8]>> {
        self.entries(child)?.get(key).map(Option::as_deref)
    }
}

/// The state at one block: the base with diffs stacked on top, oldest first.
#[derive(Clone)]
pub struct View<'a> {
    base: &'a Base,
    layers: Vec<&'a Diff>,
}

fn after(key: &[u8], inclusive: bool) -> (Bound<&[u8]>, Bound<&[u8]>) {
    let lower = if inclusive {
        Bound::Included(key)
    } else {
        Bound::Excluded(key)
    };
    (lower, Bound::Unbounded)
}

impl<'a> View<'a> {
    pub(crate) fn new(base: &'a Base, layers: impl IntoIterator<Item = &'a Diff>) -> Self {
        View {
            base,
            layers: layers.into_iter().collect(),
        }
    }

    /// This view with one more diff on top.
    pub(crate) fn with(&self, top: &'a Diff) -> Self {
        let mut view = self.clone();
        view.layers.push(top);
        view
    }

    pub fn get(&self, child: Option<&[u8]>, key: &[u8]) -> Option<&'a [u8]> {
        for layer in self.layers.iter().rev() {
            if let Some(value) = layer.get(child, key) {
                return value;
            }
        }
        self.base.entries(child)?.get(key).map(Vec::as_slice)
    }

    /// The first key `>= key` (`> key` unless `inclusive`) that has a value.
    pub fn next_key(&self, child: Option<&[u8]>, key: &[u8], inclusive: bool) -> Option<&'a [u8]> {
        let mut from = key;
        let mut inclusive = inclusive;
        loop {
            // The smallest key any layer knows of; a newer deletion is
            // filtered out by resolving its value through all layers.
            let base = self
                .base
                .entries(child)
                .and_then(|e| e.range::<[u8], _>(after(from, inclusive)).next())
                .map(|(k, _)| k.as_slice());
            let candidate = self
                .layers
                .iter()
                .filter_map(|l| {
                    l.entries(child)?
                        .range::<[u8], _>(after(from, inclusive))
                        .next()
                })
                .map(|(k, _)| k.as_slice())
                .chain(base)
                .min()?;
            if self.get(child, candidate).is_some() {
                return Some(candidate);
            }
            from = candidate;
            inclusive = false;
        }
    }

    /// All keys starting with `prefix`, in order.
    pub fn keys_with_prefix(&self, child: Option<&[u8]>, prefix: &[u8]) -> Vec<&'a [u8]> {
        let mut keys = Vec::new();
        let mut next = self.next_key(child, prefix, true);
        while let Some(key) = next.filter(|k| k.starts_with(prefix)) {
            keys.push(key);
            next = self.next_key(child, key, false);
        }
        keys
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Base {
        let mut b = Base::default();
        for k in [b"a", b"b", b"c", b"d"] {
            b.main.insert(k.to_vec(), k.to_vec());
        }
        b.child
            .insert(b"kid".to_vec(), [(b"x".to_vec(), b"1".to_vec())].into());
        b
    }

    #[test]
    fn layers_override_and_delete() {
        let b = base();
        let mut d1 = Diff::default();
        d1.remove(b"b".to_vec());
        d1.insert(b"bb".to_vec(), b"new".to_vec());
        let mut d2 = Diff::default();
        d2.insert(b"b".to_vec(), b"back".to_vec());
        d2.remove(b"c".to_vec());

        let v1 = View::new(&b, [&d1]);
        assert_eq!(v1.get(None, b"b"), None);
        assert_eq!(v1.get(None, b"bb"), Some(&b"new"[..]));
        assert_eq!(v1.next_key(None, b"a", false), Some(&b"bb"[..]));

        let v2 = v1.with(&d2);
        assert_eq!(v2.get(None, b"b"), Some(&b"back"[..]));
        assert_eq!(v2.next_key(None, b"bb", false), Some(&b"d"[..]));
        assert_eq!(
            v2.keys_with_prefix(None, b""),
            [&b"a"[..], b"b", b"bb", b"d"]
        );
        // Newer layers don't affect older views.
        assert_eq!(View::new(&b, []).get(None, b"c"), Some(&b"c"[..]));
    }

    #[test]
    fn child_tries_accept_prefixed_ids() {
        let b = base();
        let mut d = Diff::default();
        d.set(
            Some(&[DEFAULT_CHILD_STORAGE_PREFIX, b"kid"].concat()),
            b"y",
            Some(b"2"),
        );
        let v = View::new(&b, [&d]);
        assert_eq!(v.get(Some(b"kid"), b"x"), Some(&b"1"[..]));
        assert_eq!(v.keys_with_prefix(Some(b"kid"), b""), [&b"x"[..], b"y"]);
        assert_eq!(v.get(Some(b"other"), b"x"), None);
    }
}
