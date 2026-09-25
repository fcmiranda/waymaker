use crate::nucleo::Indexed;
use crate::{Identifier, Selection};
use indexmap::IndexMap;
use rustc_hash::FxBuildHasher;
use std::sync::Mutex;
use std::{borrow::Borrow, hash::Hash, sync::Arc};

pub type SelectionValidator<S> = fn(&S) -> bool;

#[derive(Debug)]
pub struct Selector<T, S> {
    selections: Option<SelectorImpl<u32, S>>,
    pub identifier: Identifier<T, S>,
    pub validator: SelectionValidator<S>,
}

pub fn truthy_validator<S>(_: &S) -> bool {
    true
}

impl<T> Selector<Indexed<T>, ()> {
    /// See [`Indexed::dummy_identifier`].
    pub fn new_disabled() -> Self {
        Self {
            selections: None,
            identifier: Indexed::dummy_identifier,
            validator: truthy_validator,
        }
    }
}

impl<T, S: Selection> Selector<T, S> {
    pub fn new(identifier: Identifier<T, S>) -> Self {
        Self {
            selections: Some(SelectorImpl::new()),
            identifier,
            validator: truthy_validator,
        }
    }

    pub fn with_validator(mut self, validator: SelectionValidator<S>) -> Self {
        self.validator = validator;
        self
    }

    pub fn disabled(mut self) -> Self {
        self.selections = None;
        self
    }

    pub fn is_disabled(&self) -> bool {
        self.selections.is_none()
    }

    pub fn id(&self, item: &T) -> u32 {
        (self.identifier)(item).0
    }
    pub fn eval(&self, item: &T) -> S {
        (self.identifier)(item).1
    }

    // --------------------------------------------

    pub fn sel(&mut self, item: &T) -> bool {
        let Some(selections) = &mut self.selections else {
            return false;
        };

        let (k, v) = (self.identifier)(item);
        selections.insert(k, v)
    }

    pub fn desel(&mut self, item: &T) -> bool {
        let Some(selections) = &mut self.selections else {
            return false;
        };

        let (k, _v) = (self.identifier)(item);
        selections.remove(&k)
    }

    pub fn contains(&self, item: &T) -> bool {
        let Some(selections) = &self.selections else {
            return false;
        };

        let (k, _v) = (self.identifier)(item);
        selections.contains(&k)
    }

    pub fn toggle(&mut self, item: &T) {
        let Some(selections) = &mut self.selections else {
            return;
        };

        let (k, v) = (self.identifier)(item);
        if selections.contains(&k) {
            selections.remove(&k);
        } else {
            selections.insert(k, v);
        }
    }

    pub fn clear(&mut self) {
        if let Some(selections) = &mut self.selections {
            selections.clear();
        }
    }

    pub fn len(&self) -> usize {
        self.selections.as_ref().map_or(0, |s| s.len())
    }

    // -----------------------------------------------------

    pub fn is_empty(&self) -> bool {
        self.selections.as_ref().is_none_or(|s| s.is_empty())
    }

    pub fn output(&mut self) -> impl Iterator<Item = S> {
        if let Some(selections) = &mut self.selections {
            let mut set = selections.set.lock().unwrap();
            std::mem::take(&mut *set).into_values()
        } else {
            IndexMap::with_capacity(0).into_values()
        }
    }

    pub fn identify_to_vec<I>(&self, items: I) -> Vec<S>
    where
        I: IntoIterator,
        I::Item: std::borrow::Borrow<T> + Send,
    {
        items
            .into_iter()
            .map(|item| (self.identifier)(item.borrow()).1)
            .collect()
    }

    pub fn map_to_vec<U, F>(&self, mut f: F) -> Vec<U>
    where
        F: FnMut(u32, &S) -> U,
    {
        self.selections
            .as_ref()
            .map_or_else(Vec::new, |s| s.map_to_vec(|(k, v)| f(*k, v)))
    }

    pub fn map_last<U, F>(&self, f: F) -> Option<U>
    where
        F: FnOnce(&S) -> U,
    {
        self.selections
            .as_ref()
            .and_then(|s| s.map_last(|(_, s)| f(s)))
    }

    pub fn revalidate(&mut self) {
        let Some(selections) = &mut self.selections else {
            return;
        };

        let mut set = selections.set.lock().unwrap();
        let validator = &self.validator;

        set.retain(|_, v| validator(v));
    }

    pub fn cycle_all_bg<I>(&self, items: I)
    where
        I: IntoIterator,
        I::Item: std::borrow::Borrow<T> + Send,
    {
        let Some(selections) = &self.selections else {
            return;
        };

        let results: Vec<_> = items
            .into_iter()
            .map(|item| (self.identifier)(item.borrow()))
            .collect();

        let selections = selections.clone();

        #[cfg(feature = "parallelism")]
        tokio::task::spawn_blocking(move || {
            let mut set_guard = selections.set.lock().unwrap();

            let mut all = true;
            let mut seen = 0;
            for (i, (k, _)) in results.iter().enumerate() {
                if !set_guard.contains_key(k) {
                    all = false;
                    seen = i;
                    break;
                }
            }

            if all {
                for (k, _) in results {
                    set_guard.swap_remove(&k);
                }
            } else {
                for (k, v) in results.into_iter().skip(seen) {
                    set_guard.insert(k, v);
                }
            }
        });

        #[cfg(not(feature = "parallelism"))]
        {
            let mut set_guard = selections.set.lock().unwrap();

            let mut all = true;
            let mut seen = 0;
            for (i, (k, _)) in results.iter().enumerate() {
                if !set_guard.contains_key(k) {
                    all = false;
                    seen = i;
                    break;
                }
            }

            if all {
                for (k, _) in results {
                    set_guard.swap_remove(&k);
                }
            } else {
                for (k, v) in results.into_iter().skip(seen) {
                    set_guard.insert(k, v);
                }
            }
        }
    }

    // ---------- wrappers (?) -----------

    // pub fn insert(&mut self, (k, v): (u32, S)) -> bool {
    //     let Some(selections) = &mut self.selections else {
    //         return false;
    //     };
    //     selections.insert(k, v)
    // }
}

// ---------- Selection Set ---------------
#[derive(Debug, Clone)]
struct SelectorImpl<K: Eq + Hash, S> {
    pub set: Arc<Mutex<IndexMap<K, S, FxBuildHasher>>>,
}

impl<K: Eq + Hash + Clone, S> SelectorImpl<K, S>
where
    S: Selection,
{
    // pub fn indices(&self) -> Vec<K> {
    //     self.set
    //         .lock()
    //         .unwrap()
    //         .iter()
    //         .enumerate()
    //         .map(|(_, (k, _))| k.clone())
    //         .collect()
    // }

    pub fn new() -> Self {
        Self {
            set: Arc::new(Mutex::new(IndexMap::with_hasher(FxBuildHasher))),
        }
    }

    pub fn insert(&self, key: K, value: S) -> bool {
        let mut set = self.set.lock().unwrap();
        set.insert(key, value).is_none()
    }

    pub fn remove(&self, key: &K) -> bool {
        let mut set = self.set.lock().unwrap();
        set.shift_remove(key).is_some()
    }

    pub fn contains(&self, key: &K) -> bool {
        let set = self.set.lock().unwrap();
        set.contains_key(key)
    }

    pub fn clear(&self) {
        let mut set = self.set.lock().unwrap();
        set.clear();
    }

    pub fn clone(&self) -> Self {
        Self {
            set: Arc::clone(&self.set),
        }
    }

    pub fn len(&self) -> usize {
        let set = self.set.lock().unwrap();
        set.len()
    }

    pub fn is_empty(&self) -> bool {
        let set = self.set.lock().unwrap();
        set.is_empty()
    }

    pub fn map_to_vec<U, F>(&self, f: F) -> Vec<U>
    where
        F: FnMut((&K, &S)) -> U,
    {
        let set = self.set.lock().unwrap();
        set.iter().map(f).collect()
    }

    pub fn map_last<U>(&self, f: impl FnOnce((&K, &S)) -> U) -> Option<U> {
        let set = self.set.lock().unwrap();
        set.last().map(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_selector_basic_operations() {
        let mut selector = Selector::new(|s: &String| (s.len() as u32, s.clone()));
        assert!(!selector.is_disabled());
        assert!(selector.is_empty());
        assert_eq!(selector.len(), 0);

        let apple = "apple".to_string();
        let banana = "banana".to_string();

        assert_eq!(selector.id(&apple), 5);
        assert_eq!(selector.eval(&apple), "apple");

        assert!(selector.sel(&apple));
        assert!(selector.contains(&apple));
        assert!(!selector.contains(&banana));
        assert_eq!(selector.len(), 1);
        assert!(!selector.is_empty());

        // Toggle removes apple
        selector.toggle(&apple);
        assert!(!selector.contains(&apple));
        assert_eq!(selector.len(), 0);

        // Toggle adds banana
        selector.toggle(&banana);
        assert!(selector.contains(&banana));
        assert_eq!(selector.len(), 1);

        // Desel removes banana
        assert!(selector.desel(&banana));
        assert!(!selector.contains(&banana));
        assert_eq!(selector.len(), 0);

        // Clear removes all
        selector.sel(&apple);
        selector.sel(&banana);
        assert_eq!(selector.len(), 2);
        selector.clear();
        assert_eq!(selector.len(), 0);
    }

    #[test]
    fn test_selector_disabled() {
        let mut sel = Selector::new(|s: &String| (0u32, s.clone())).disabled();
        assert!(sel.is_disabled());

        let item = "sample".to_string();
        assert!(!sel.sel(&item));
        assert!(!sel.contains(&item));
        assert!(!sel.desel(&item));
        sel.toggle(&item);
        assert_eq!(sel.len(), 0);
        assert!(sel.is_empty());
        assert!(sel.output().next().is_none());
        assert!(sel.map_to_vec(|_, _| ()).is_empty());
        assert!(sel.map_last(|_| ()).is_none());
        sel.cycle_all_bg(vec![item]);
    }

    #[test]
    fn test_selector_mapping_and_output() {
        let mut sel = Selector::new(|(id, val): &(u32, String)| (*id, val.clone()));
        let item1 = (1u32, "first".to_string());
        let item2 = (2u32, "second".to_string());

        sel.sel(&item1);
        sel.sel(&item2);

        let mapped: Vec<String> = sel.map_to_vec(|_k, v| v.clone());
        assert_eq!(mapped, vec!["first", "second"]);

        let last = sel.map_last(|s| s.clone());
        assert_eq!(last, Some("second".to_string()));

        let identified = sel.identify_to_vec(vec![&item1, &item2]);
        assert_eq!(identified, vec!["first", "second"]);

        let output_vals: Vec<String> = sel.output().collect();
        assert_eq!(output_vals, vec!["first", "second"]);
        assert!(sel.is_empty());
    }

    #[test]
    fn test_selector_validator_and_revalidate() {
        let mut sel = Selector::new(|s: &String| (s.len() as u32, s.clone()))
            .with_validator(|s| !s.starts_with('_'));

        let valid = "valid.txt".to_string();
        let invalid = "_hidden.txt".to_string();

        sel.sel(&valid);
        sel.sel(&invalid);
        assert_eq!(sel.len(), 2);

        sel.revalidate();
        assert_eq!(sel.len(), 1);
        assert!(sel.contains(&valid));
        assert!(!sel.contains(&invalid));
    }

    #[test]
    fn test_selector_cycle_all() {
        let sel = Selector::new(|(id, s): &(u32, String)| (*id, s.clone()));
        let items = vec![
            (1u32, "one".to_string()),
            (2u32, "two".to_string()),
            (3u32, "three".to_string()),
        ];

        // None are selected -> cycle_all selects all
        sel.cycle_all_bg(&items);
        assert_eq!(sel.len(), 3);
        assert!(sel.contains(&items[0]));
        assert!(sel.contains(&items[1]));
        assert!(sel.contains(&items[2]));

        // All are selected -> cycle_all deselects all
        sel.cycle_all_bg(&items);
        assert_eq!(sel.len(), 0);
    }
}
