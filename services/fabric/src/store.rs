//! The object store and its incremental indexes. [`ObjectStore`] is the
//! horizontally-scalable seam (ports-and-adapters): a distributed backend
//! (FoundationDB/ScyllaDB-class) implements the same trait later. [`InMemoryStore`]
//! is the reference implementation and keeps a **type index** and an **inverted
//! property index** current on every write — incremental indexing, so object
//! queries stay fast as data mutates. [DD-1]

use std::collections::{BTreeMap, BTreeSet};

use crate::object::{LinkInstance, ObjectInstance};

/// One store mutation, for applying several at once with [`ObjectStore::try_apply_all`].
#[derive(Debug, Clone, PartialEq)]
pub enum StoreOp {
    /// Insert or replace an object.
    PutObject(ObjectInstance),
    /// Insert a link.
    PutLink(LinkInstance),
    /// Remove an object and its links; nothing happens when there is no such object.
    RemoveObject(String),
}

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
    /// Remove an object with its index entries and every link from or to it, returning
    /// it; `None` (and no change) when there is no such object. Used by retention.
    fn remove_object(&mut self, id: &str) -> Option<ObjectInstance>;
    /// Apply `ops` in order, reporting a persistence failure. A durable store makes
    /// them durable together, with one flush, and on failure applies none of them.
    fn try_apply_all(&mut self, ops: Vec<StoreOp>) -> Result<(), crate::FabricError> {
        for op in ops {
            match op {
                StoreOp::PutObject(o) => self.put_object(o),
                StoreOp::PutLink(l) => self.put_link(l),
                StoreOp::RemoveObject(id) => {
                    self.remove_object(&id);
                }
            }
        }
        Ok(())
    }
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
    fn remove_object(&mut self, id: &str) -> Option<ObjectInstance> {
        (**self).remove_object(id)
    }
    fn try_apply_all(&mut self, ops: Vec<StoreOp>) -> Result<(), crate::FabricError> {
        (**self).try_apply_all(ops)
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
    // target_id -> (source_id, link_type): the reverse of `adjacency`, so removing an
    // object drops the links into it without a scan
    incoming: BTreeMap<String, BTreeSet<(String, String)>>,
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

    /// Every object, in id order.
    pub fn objects(&self) -> impl Iterator<Item = &ObjectInstance> {
        self.objects.values()
    }

    /// Every link, grouped by source and link type, each group in insertion order.
    pub fn links(&self) -> impl Iterator<Item = LinkInstance> + '_ {
        self.adjacency
            .iter()
            .flat_map(|((source, link_type), targets)| {
                targets.iter().map(move |target| LinkInstance {
                    link_type: link_type.clone(),
                    source_id: source.clone(),
                    target_id: target.clone(),
                })
            })
    }

    /// How many objects and links the store holds.
    pub fn len(&self) -> (usize, usize) {
        (
            self.objects.len(),
            self.adjacency.values().map(Vec::len).sum(),
        )
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
        self.incoming
            .entry(link.target_id.clone())
            .or_default()
            .insert((link.source_id.clone(), link.link_type.clone()));
        self.adjacency
            .entry((link.source_id, link.link_type))
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

    fn remove_object(&mut self, id: &str) -> Option<ObjectInstance> {
        let obj = self.objects.remove(id)?;
        self.deindex(&obj);
        let outgoing: Vec<(String, String)> = self
            .adjacency
            .range((id.to_string(), String::new())..)
            .take_while(|((source, _), _)| source == id)
            .map(|(k, _)| k.clone())
            .collect();
        for key in outgoing {
            for target in self.adjacency.remove(&key).unwrap_or_default() {
                if let Some(set) = self.incoming.get_mut(&target) {
                    set.remove(&key);
                }
            }
        }
        for (source, link_type) in self.incoming.remove(id).unwrap_or_default() {
            if let Some(targets) = self.adjacency.get_mut(&(source, link_type)) {
                targets.retain(|t| t != id);
            }
        }
        Some(obj)
    }
}
