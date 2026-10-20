//! Retention windows: how long an object type's objects are kept after they were last
//! current. The governance layer that hosts the fabric enforces them, auditing each
//! removal before it is journalled. [SAD_00 §8.5] [NFR-SEC-2]
//!
//! An object's age is measured from the **latest** of the type's named timestamp
//! properties, so a record that is refreshed (a newer `synced_at`) or closed (a
//! `cleared_at`) restarts its window. An object holding none of them has no known age
//! and is kept.

use crate::store::ObjectStore;
use crate::value::Value;

/// One object type's retention window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Retention {
    /// The object type it governs.
    pub object_type: String,
    /// The timestamp properties an object's age is measured from (the latest present).
    pub after: Vec<String>,
    /// How long, in seconds, an object is kept after that time.
    pub seconds: u64,
}

impl Retention {
    /// When `obj` was last current: the latest of its `after` properties.
    pub fn current_at(&self, obj: &crate::object::ObjectInstance) -> Option<u64> {
        self.after
            .iter()
            .filter_map(|p| match obj.properties.get(p) {
                Some(Value::Timestamp(t)) => Some(*t),
                Some(Value::Int(i)) => u64::try_from(*i).ok(),
                _ => None,
            })
            .max()
    }

    /// The ids of `store`'s objects of this type whose window has passed at `now`, at
    /// most `limit` of them, oldest first.
    pub fn expired<S: ObjectStore + ?Sized>(
        &self,
        store: &S,
        now: u64,
        limit: usize,
    ) -> Vec<String> {
        let mut due: Vec<(u64, &str)> = store
            .objects_of_type(&self.object_type)
            .into_iter()
            .filter_map(|o| self.current_at(o).map(|at| (at, o.id.as_str())))
            .filter(|(at, _)| now.saturating_sub(*at) > self.seconds)
            .collect();
        due.sort_unstable();
        due.into_iter()
            .take(limit)
            .map(|(_, id)| id.to_string())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::{LinkInstance, ObjectInstance};
    use crate::store::InMemoryStore;
    use policy::{ClassificationLevel, Marking};
    use std::collections::{BTreeMap, BTreeSet};

    fn obj(id: &str, props: &[(&str, Value)]) -> ObjectInstance {
        ObjectInstance {
            id: id.into(),
            type_id: "track".into(),
            properties: props
                .iter()
                .map(|(k, v)| (k.to_string(), v.clone()))
                .collect::<BTreeMap<_, _>>(),
            marking: Marking {
                level: ClassificationLevel::UNCLASSIFIED,
                compartments: BTreeSet::new(),
            },
            allowed_purposes: BTreeSet::new(),
        }
    }

    #[test]
    fn age_runs_from_the_latest_named_time_and_unknown_ages_are_kept() {
        let rule = Retention {
            object_type: "track".into(),
            after: vec!["synced_at".into(), "last_seen".into()],
            seconds: 100,
        };
        let mut store = InMemoryStore::new();
        store.put_object(obj("track/old", &[("last_seen", Value::Timestamp(10))]));
        store.put_object(obj(
            "track/refreshed",
            &[
                ("last_seen", Value::Timestamp(10)),
                ("synced_at", Value::Timestamp(950)),
            ],
        ));
        store.put_object(obj("track/ageless", &[]));
        store.put_object(obj("track/older", &[("last_seen", Value::Timestamp(5))]));
        assert_eq!(
            rule.expired(&store, 1000, 10),
            vec!["track/older", "track/old"]
        );
        assert_eq!(rule.expired(&store, 1000, 1), vec!["track/older"]);
        assert!(rule.expired(&store, 105, 10).is_empty());
    }

    #[test]
    fn removing_an_object_drops_its_indexes_and_links_both_ways() {
        let mut store = InMemoryStore::new();
        store.put_object(obj("track/a", &[("name", Value::Text("A".into()))]));
        store.put_object(obj("track/b", &[]));
        store.put_object(obj("track/c", &[]));
        let link = |s: &str, t: &str| LinkInstance {
            link_type: "near".into(),
            source_id: s.into(),
            target_id: t.into(),
        };
        store.put_link(link("track/a", "track/b"));
        store.put_link(link("track/c", "track/a"));
        store.put_link(link("track/c", "track/b"));
        assert_eq!(
            store.remove_object("track/a").map(|o| o.id),
            Some("track/a".into())
        );
        assert!(store.remove_object("track/a").is_none());
        assert!(store.get_object("track/a").is_none());
        assert!(store.by_property("track", "name", "A").is_empty());
        assert_eq!(store.objects_of_type("track").len(), 2);
        assert!(store.neighbors("track/a", "near").is_empty());
        let from_c: Vec<_> = store
            .neighbors("track/c", "near")
            .iter()
            .map(|o| o.id.clone())
            .collect();
        assert_eq!(from_c, vec!["track/b"]);
        // Put back, it starts with no links.
        store.put_object(obj("track/a", &[]));
        assert!(store.neighbors("track/a", "near").is_empty());
    }
}
