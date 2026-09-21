//! The object store and its incremental indexes. [`ObjectStore`] is the
//! horizontally-scalable seam (ports-and-adapters): a distributed backend
//! (FoundationDB/ScyllaDB-class) implements the same trait later. [`InMemoryStore`]
//! is the reference implementation and keeps a **type index** and an **inverted
//! property index** current on every write — incremental indexing, so object
//! queries stay fast as data mutates. [DD-1]

use std::collections::{BTreeMap, BTreeSet};

use crate::object::{LinkInstance, ObjectInstance};

/// Storage + retrieval + adjacency over objects and links. Object-safe so the
/// query and action layers depend on the trait, not a concrete store.
pub trait ObjectStore {
    /// Insert or replace an object, keeping all indexes current.
    fn put_object(&mut self, obj: ObjectInstance);
    /// Fetch an object by id.
    fn get_object(&self, id: &str) -> Option<&ObjectInstance>;
    /// All objects of a type (via the type index).
    fn objects_of_type(&self, type_id: &str) -> Vec<&ObjectInstance>;
    /// Objects of a type whose property equals a canonical key (inverted index).
    fn by_property(&self, type_id: &str, property: &str, key: &str) -> Vec<&ObjectInstance>;
    /// Insert a link (directed edge).
    fn put_link(&mut self, link: LinkInstance);
    /// Objects reachable from `source_id` over `link_type` (search-around).
    fn neighbors(&self, source_id: &str, link_type: &str) -> Vec<&ObjectInstance>;
}

/// A boxed store is a store, so a service can pick its backend (in-memory or
/// durable) at startup and still hold one concrete fabric type. [DD-1]
impl<T: ObjectStore + ?Sized> ObjectStore for Box<T> {
    fn put_object(&mut self, obj: ObjectInstance) {
        (**self).put_object(obj)
    }
    fn get_object(&self, id: &str) -> Option<&ObjectInstance> {
        (**self).get_object(id)
    }
    fn objects_of_type(&self, type_id: &str) -> Vec<&ObjectInstance> {
        (**self).objects_of_type(type_id)
    }
    fn by_property(&self, type_id: &str, property: &str, key: &str) -> Vec<&ObjectInstance> {
        (**self).by_property(type_id, property, key)
    }
    fn put_link(&mut self, link: LinkInstance) {
        (**self).put_link(link)
    }
    fn neighbors(&self, source_id: &str, link_type: &str) -> Vec<&ObjectInstance> {
        (**self).neighbors(source_id, link_type)
    }
}

/// A non-durable, fully-indexed in-memory store.
#[derive(Debug, Default)]
pub struct InMemoryStore {
    objects: BTreeMap<String, ObjectInstance>,
    type_index: BTreeMap<String, BTreeSet<String>>,
    // (type_id, property, canonical value) -> object ids
    prop_index: BTreeMap<(String, String, String), BTreeSet<String>>,
    // (source_id, link_type) -> target ids
    adjacency: BTreeMap<(String, String), Vec<String>>,
}

impl InMemoryStore {
    /// A new empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Remove an existing object's index entries (before re-indexing on update).
    fn deindex(&mut self, obj: &ObjectInstance) {
        if let Some(set) = self.type_index.get_mut(&obj.type_id) {
            set.remove(&obj.id);
        }
        for (prop, value) in &obj.properties {
            let key = (obj.type_id.clone(), prop.clone(), value.as_index_key());
            if let Some(set) = self.prop_index.get_mut(&key) {
                set.remove(&obj.id);
            }
        }
    }

    fn index(&mut self, obj: &ObjectInstance) {
        self.type_index
            .entry(obj.type_id.clone())
            .or_default()
            .insert(obj.id.clone());
        for (prop, value) in &obj.properties {
            let key = (obj.type_id.clone(), prop.clone(), value.as_index_key());
            self.prop_index
                .entry(key)
                .or_default()
                .insert(obj.id.clone());
        }
    }

    fn resolve(&self, ids: &BTreeSet<String>) -> Vec<&ObjectInstance> {
        ids.iter().filter_map(|id| self.objects.get(id)).collect()
    }
}

impl ObjectStore for InMemoryStore {
    fn put_object(&mut self, obj: ObjectInstance) {
        if let Some(old) = self.objects.get(&obj.id).cloned() {
            self.deindex(&old);
        }
        self.index(&obj);
        self.objects.insert(obj.id.clone(), obj);
    }

    fn get_object(&self, id: &str) -> Option<&ObjectInstance> {
        self.objects.get(id)
    }

    fn objects_of_type(&self, type_id: &str) -> Vec<&ObjectInstance> {
        match self.type_index.get(type_id) {
            Some(ids) => self.resolve(ids),
            None => Vec::new(),
        }
    }

    fn by_property(&self, type_id: &str, property: &str, key: &str) -> Vec<&ObjectInstance> {
        let k = (type_id.to_string(), property.to_string(), key.to_string());
        match self.prop_index.get(&k) {
            Some(ids) => self.resolve(ids),
            None => Vec::new(),
        }
    }

    fn put_link(&mut self, link: LinkInstance) {
        self.adjacency
            .entry((link.source_id.clone(), link.link_type.clone()))
            .or_default()
            .push(link.target_id);
    }

    fn neighbors(&self, source_id: &str, link_type: &str) -> Vec<&ObjectInstance> {
        match self
            .adjacency
            .get(&(source_id.to_string(), link_type.to_string()))
        {
            Some(targets) => targets
                .iter()
                .filter_map(|id| self.objects.get(id))
                .collect(),
            None => Vec::new(),
        }
    }
}
