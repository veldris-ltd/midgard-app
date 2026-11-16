//! The client's state on disk: the delivered rows and the pending queue, sealed by
//! the core under a root the platform key root seals. Nothing of the dataset is
//! readable in the file; withheld rows are kept as their level only; a persona
//! switch leaves nothing of the previous persona; any change to the file loads
//! nothing; a requeued action gets the decision the enclosure already made, and
//! the clock, the action counter and the file's generation survive a relaunch.
//! [AC-C-2] [AC-C-13] [FR-C-2] [FR-C-4]

use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::path::PathBuf;

use attest::json::{parse, Json};
use midgard_client_core::demo::enclosure::DemoEnclosure;
use midgard_client_core::keystore::sealed_root::{RootSealer, SealedRootKeystore};
use midgard_client_core::keystore::{KeystoreError, SecurityFloor};
use midgard_client_core::source::{action, DataSource, Delivered, Grant};
use midgard_client_core::sync::SyncOp;
use midgard_client_core::{ClientCore, CoreError, CORE_VERSION, PENDING_OBJECT};
use pqc_agility::backend::fips::provider;
use pqc_agility::{Aes256GcmAead, OsRng, Rng, SecretBytes, GCM_NONCE_LEN};

/// A software stand-in for the Secure Enclave key root.
struct TestSealer([u8; 32]);
impl RootSealer for TestSealer {
    fn seal(&self, data_key: &SecretBytes) -> Result<Vec<u8>, KeystoreError> {
        let mut nonce = [0u8; GCM_NONCE_LEN];
        OsRng.fill(&mut nonce);
        let mut out = nonce.to_vec();
        out.extend(
            Aes256GcmAead::new(&self.0)
                .seal(&nonce, b"", data_key.expose())
                .unwrap(),
        );
        Ok(out)
    }
    fn unseal(&self, sealed: &[u8]) -> Result<SecretBytes, KeystoreError> {
        let nonce: [u8; GCM_NONCE_LEN] = sealed[..GCM_NONCE_LEN].try_into().unwrap();
        Aes256GcmAead::new(&self.0)
            .open(&nonce, b"", &sealed[GCM_NONCE_LEN..])
            .map(SecretBytes::new)
            .map_err(|_| KeystoreError::Unseal)
    }
    fn hardware_backed(&self) -> bool {
        false
    }
}

const SEALER: TestSealer = TestSealer([3; 32]);

fn enclosure() -> DemoEnclosure {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../demo/datasets/{CORE_VERSION}.mgdemo"));
    DemoEnclosure::open_pinned(&fs::read(path).unwrap()).unwrap()
}

fn new_root() -> Vec<u8> {
    SealedRootKeystore::create(&provider(), "device-root", &SEALER, &mut OsRng)
        .unwrap()
        .1
}

fn core(root: &[u8], grant: &Grant) -> ClientCore<SealedRootKeystore> {
    let ks = SealedRootKeystore::open(root, &SEALER).unwrap();
    ClientCore::open_scoped(
        provider(),
        ks,
        grant.clearance().clone(),
        SecurityFloor::NonExtractable,
        "dev-1",
        &grant.persona().id,
    )
    .unwrap()
}

fn cache_everything(e: &DemoEnclosure, c: &mut ClientCore<SealedRootKeystore>, g: &Grant) {
    for (name, _) in e.collections() {
        let rows = e.fetch(g, &name).unwrap();
        c.cache_delivery(&name, &rows, &mut OsRng).unwrap();
    }
}

fn own_marking(g: &Grant) -> policy::Marking {
    policy::Marking {
        level: g.clearance().level,
        compartments: g.clearance().compartments.clone(),
    }
}

/// Queue a `demo.ping` on `target` as action `id`.
fn queue_ping(c: &mut ClientCore<SealedRootKeystore>, g: &Grant, id: &str, target: &str) {
    for (f, v) in [
        (action::VERB, "demo.ping"),
        (action::TARGET, target),
        (action::PARAMS, "{}"),
        (action::BY, g.persona().id.as_str()),
    ] {
        c.edit_field(id, f, v.as_bytes().to_vec(), own_marking(g), &mut OsRng)
            .unwrap();
    }
}

/// The first row `g` is shown, in collection order, as `<collection>/<id>`.
fn first_shown(e: &DemoEnclosure, g: &Grant) -> String {
    e.collections()
        .into_iter()
        .find_map(|(name, _)| {
            e.fetch(g, &name)
                .unwrap()
                .into_iter()
                .find_map(|d| match d {
                    Delivered::Row { id, .. } => Some(format!("{name}/{id}")),
                    _ => None,
                })
        })
        .unwrap()
}

/// Every string value in `v`, recursively.
fn strings(v: &Json, out: &mut BTreeSet<String>) {
    match v {
        Json::Str(s) => {
            out.insert(s.clone());
        }
        Json::Arr(items) => items.iter().for_each(|x| strings(x, out)),
        Json::Obj(pairs) => pairs.iter().for_each(|(_, x)| strings(x, out)),
        _ => {}
    }
}

#[test]
fn the_state_file_on_disk_holds_nothing_readable_from_the_dataset() {
    let e = enclosure();
    // What must not be readable: every persona's id and name, every collection
    // name, every row id, every marking word and compartment, and every string of
    // eight bytes or more in any row body.
    let mut needles = BTreeSet::new();
    for p in e.personas() {
        needles.insert(p.id.clone());
        needles.insert(p.display.clone());
        needles.extend(p.compartments.iter().cloned());
    }
    let top = e.grant("k.osei").unwrap();
    for (name, _) in e.collections() {
        needles.insert(name.clone());
        for d in e.fetch(&top, &name).unwrap() {
            if let Delivered::Row {
                id, marking, body, ..
            } = d
            {
                needles.insert(id);
                needles.extend(marking.compartments().iter().cloned());
                let mut found = BTreeSet::new();
                strings(
                    &parse(std::str::from_utf8(&body).unwrap()).unwrap(),
                    &mut found,
                );
                needles.extend(found.into_iter().filter(|s| s.len() >= 8));
            }
        }
    }
    for w in [
        "restricted",
        "internal",
        "confidential",
        "sys/pending",
        "demo.ping",
    ] {
        needles.insert(w.to_string());
    }
    let needles: Vec<String> = needles.into_iter().filter(|n| n.len() >= 5).collect();
    assert!(needles.len() > 200, "only {} needles", needles.len());

    let dir = std::env::temp_dir().join(format!("midgard-cache-test-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let root = new_root();
    for p in e.personas() {
        let g = e.grant(&p.id).unwrap();
        let mut c = core(&root, &g);
        cache_everything(&e, &mut c, &g);
        let target = first_shown(&e, &g);
        queue_ping(&mut c, &g, "action:dev-1-1", &target);
        let path = dir.join("state.bin");
        fs::write(&path, c.export_state(&mut OsRng).unwrap().file).unwrap();
        let file = fs::read(&path).unwrap();
        fs::remove_file(&path).unwrap();
        // All five-byte windows of the file; a needle is looked for in full only
        // when its first five bytes occur.
        let windows: HashSet<&[u8]> = file.windows(5).collect();
        for n in &needles {
            let n = n.as_bytes();
            if windows.contains(&n[..5]) {
                assert!(
                    !file.windows(n.len()).any(|w| w == n),
                    "{:?} is readable in {}'s state file",
                    String::from_utf8_lossy(n),
                    p.id
                );
            }
        }
        // The same file reloads in full for the same persona and root.
        let mut again = core(&root, &g);
        again.import_state(&file, 0).unwrap();
        assert_eq!(again.pending_ops().len(), 4);
        for (name, _) in e.collections() {
            assert_eq!(
                again.cached_delivery(&name).unwrap(),
                e.fetch(&g, &name).unwrap(),
                "{name} for {}",
                p.id
            );
        }
    }
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn withheld_rows_are_cached_as_their_level_only() {
    let e = enclosure();
    let low = e.grant("k.rao").unwrap();
    let top = e.grant("k.osei").unwrap();
    let mut c = core(&new_root(), &low);
    let mut checked = 0;
    for (name, _) in e.collections() {
        let rows = e.fetch(&low, &name).unwrap();
        let withheld = rows
            .iter()
            .filter(|d| matches!(d, Delivered::Withheld { .. }))
            .count();
        if withheld == 0 {
            continue;
        }
        checked += 1;
        c.cache_delivery(&name, &rows, &mut OsRng).unwrap();
        assert_eq!(c.cached_delivery(&name).unwrap(), rows);
        // What k.osei sees and k.rao does not: none of it is cached, not even an id.
        let shown_low: BTreeSet<String> = rows
            .iter()
            .filter_map(|d| match d {
                Delivered::Row { id, .. } => Some(id.clone()),
                _ => None,
            })
            .collect();
        let hidden: Vec<String> = e
            .fetch(&top, &name)
            .unwrap()
            .into_iter()
            .filter_map(|d| match d {
                Delivered::Row { id, .. } if !shown_low.contains(&id) => Some(id),
                _ => None,
            })
            .collect();
        let cached = c.visible_objects();
        assert_eq!(
            cached
                .iter()
                .filter(|k| k.starts_with(&format!("{name}/")))
                .count(),
            shown_low.len()
        );
        let index = String::from_utf8(c.load_cached(&format!("{name}#index")).unwrap()).unwrap();
        for id in &hidden {
            assert!(!cached.contains(&format!("{name}/{id}")));
            assert!(!index.contains(&format!("\"{id}\"")), "{id} in the index");
        }
        // An index entry for a withheld row is its level and nothing else.
        let parsed = parse(&index).unwrap();
        let withheld_entries: Vec<&Json> = parsed
            .as_array()
            .unwrap()
            .iter()
            .filter(|x| x.get("withheld").is_some())
            .collect();
        assert_eq!(withheld_entries.len(), withheld);
        for x in withheld_entries {
            assert!(matches!(x, Json::Obj(p) if p.len() == 1));
        }
    }
    assert!(checked > 0, "no collection has rows withheld from k.rao");
}

#[test]
fn a_persona_switch_leaves_nothing_of_the_previous_persona() {
    let e = enclosure();
    let ben = e.grant("j.benjamin").unwrap();
    // k.osei dominates j.benjamin (same level, more compartments): a switch up.
    let osei = e.grant("k.osei").unwrap();
    let root = new_root();
    let mut c = core(&root, &ben);
    cache_everything(&e, &mut c, &ben);
    let target = first_shown(&e, &ben);
    queue_ping(&mut c, &ben, "action:dev-1-1", &target);
    let file = c.export_state(&mut OsRng).unwrap().file;

    // Refused while j.benjamin's action is unsent; nothing changes.
    let (mut c, err) = match c.rescope(&osei.persona().id, osei.clearance().clone()) {
        Err(pair) => pair,
        Ok(_) => panic!("switched with pending ops"),
    };
    assert!(matches!(err, CoreError::PendingOps(4)));
    assert_eq!(c.scope(), "j.benjamin");
    c.drain_pending();

    let (c, dropped) = c
        .rescope(&osei.persona().id, osei.clearance().clone())
        .map_err(|(_, e)| e)
        .unwrap();
    assert!(dropped > 0);
    assert!(c.visible_objects().is_empty());
    assert_eq!(c.read_field("action:dev-1-1", action::VERB), None);
    for (name, _) in e.collections() {
        assert!(c.cached_delivery(&name).is_err(), "{name} still cached");
    }

    // j.benjamin's file does not open for k.osei, on the same device root.
    let mut other = core(&root, &osei);
    assert!(other.import_state(&file, 0).is_err());
    assert!(other.visible_objects().is_empty());
    assert!(other.pending_ops().is_empty());
}

#[test]
fn a_changed_state_file_loads_nothing() {
    let e = enclosure();
    let g = e.grant("a.holt").unwrap();
    let root = new_root();
    let mut c = core(&root, &g);
    cache_everything(&e, &mut c, &g);
    let target = first_shown(&e, &g);
    queue_ping(&mut c, &g, "action:dev-1-1", &target);
    let file = c.export_state(&mut OsRng).unwrap().file;
    let flips = [0, 5, 40, file.len() / 3, file.len() / 2, file.len() - 1];
    let cuts = [0, 10, file.len() / 2, file.len() - 1];
    let mut bad: Vec<Vec<u8>> = flips
        .iter()
        .map(|&i| {
            let mut f = file.clone();
            f[i] ^= 0x01;
            f
        })
        .collect();
    bad.extend(cuts.iter().map(|&n| file[..n].to_vec()));
    for (i, f) in bad.iter().enumerate() {
        let mut fresh = core(&root, &g);
        assert!(fresh.import_state(f, 0).is_err(), "variant {i} accepted");
        assert!(
            fresh.visible_objects().is_empty(),
            "variant {i} loaded rows"
        );
        assert!(fresh.pending_ops().is_empty(), "variant {i} loaded ops");
    }
    // Another device root (same key id) cannot open it either.
    let mut elsewhere = core(&new_root(), &g);
    assert!(elsewhere.import_state(&file, 0).is_err());
    assert!(elsewhere.visible_objects().is_empty());
}

#[test]
fn a_requeued_action_gets_the_decision_already_made() {
    let mut e = enclosure();
    let g = e.grant("k.rao").unwrap();
    let root = new_root();
    let mut c = core(&root, &g);
    let target = first_shown(&e, &g);
    queue_ping(&mut c, &g, "action:dev-1-1", &target);
    // The state is written with the action queued; then it is pushed, decided, and
    // the reply is lost before the queue could be written again.
    let file = c.export_state(&mut OsRng).unwrap().file;
    let lost = e.exchange(&g, &c.drain_pending_encoded()).unwrap();
    let audit_rows = |e: &DemoEnclosure| e.fetch(&g, "midgard.audit").unwrap().len();
    let audited = audit_rows(&e);
    let lost_audit = lost
        .iter()
        .map(|b| SyncOp::decode(b).unwrap())
        .find(|o| o.field == action::AUDIT)
        .and_then(|o| o.value)
        .unwrap();

    // Relaunch: the queue comes back with its original versions, once even if the
    // file is loaded twice, and the re-push is answered with the stored decision.
    let mut relaunched = core(&root, &g);
    relaunched.import_state(&file, 0).unwrap();
    relaunched.import_state(&file, 0).unwrap();
    assert_eq!(relaunched.pending_ops().len(), 4);
    let back = e.exchange(&g, &relaunched.drain_pending_encoded()).unwrap();
    assert_eq!(back, lost, "the same ops, with the same versions");
    relaunched.merge_remote_encoded(&back).unwrap();
    assert_eq!(
        relaunched.read_field("action:dev-1-1", action::STATUS),
        Some(b"applied".to_vec())
    );
    assert_eq!(
        relaunched.read_field("action:dev-1-1", action::AUDIT),
        Some(lost_audit)
    );
    assert_eq!(audit_rows(&e), audited, "nothing ran twice");
}

/// Queue a `demo.ping` under a new action id from the core; returns the id.
fn new_ping(c: &mut ClientCore<SealedRootKeystore>, g: &Grant, target: &str) -> String {
    let id = c.new_action_id(&mut OsRng);
    queue_ping(c, g, &id, target);
    id
}

fn decode(ops: &[Vec<u8>]) -> Vec<SyncOp> {
    ops.iter().map(|b| SyncOp::decode(b).unwrap()).collect()
}

#[test]
fn the_clock_and_the_action_counter_survive_a_relaunch_with_an_empty_queue() {
    let mut e = enclosure();
    let g = e.grant("k.rao").unwrap();
    let root = new_root();
    let target = first_shown(&e, &g);
    let mut c = core(&root, &g);
    let first = new_ping(&mut c, &g, &target);
    let pushed = c.pending_ops().to_vec();
    let back = e.exchange(&g, &c.drain_pending_encoded()).unwrap();
    c.merge_remote_encoded(&back).unwrap();
    let earlier: Vec<SyncOp> = pushed.into_iter().chain(decode(&back)).collect();
    let file = c.export_state(&mut OsRng).unwrap().file; // nothing queued

    let mut relaunched = core(&root, &g);
    relaunched.import_state(&file, 0).unwrap();
    assert!(relaunched.pending_ops().is_empty());
    let second = new_ping(&mut relaunched, &g, &target);
    assert_ne!(second, first, "an action id is never reused");
    let number = |id: &str| id.rsplit('-').next().unwrap().parse::<u64>().unwrap();
    assert!(number(&second) > number(&first), "the counter came back");
    let max_earlier = earlier.iter().map(|o| o.version.clone()).max().unwrap();
    for op in relaunched.pending_ops() {
        assert!(
            op.version > max_earlier,
            "a version was reused or went back"
        );
    }
    // A fresh decision: its own audit row, not the stored answer to the first.
    let audited = e.fetch(&g, "midgard.audit").unwrap().len();
    let back = e.exchange(&g, &relaunched.drain_pending_encoded()).unwrap();
    relaunched.merge_remote_encoded(&back).unwrap();
    assert_eq!(
        relaunched.read_field(&second, action::STATUS),
        Some(b"applied".to_vec())
    );
    assert_eq!(e.fetch(&g, "midgard.audit").unwrap().len(), audited + 1);
}

#[test]
fn action_ids_differ_across_launches_even_without_a_state_file() {
    // A discarded file (tamper, rollback, reset) restores no counter; the per-launch
    // nonce still keeps the next id from naming an action already decided.
    let e = enclosure();
    let g = e.grant("k.rao").unwrap();
    let root = new_root();
    let mut ids = BTreeSet::new();
    for _ in 0..8 {
        let mut c = core(&root, &g);
        assert!(ids.insert(c.new_action_id(&mut OsRng)));
    }
}

#[test]
fn an_older_state_file_is_refused_as_a_rollback() {
    let e = enclosure();
    let g = e.grant("k.rao").unwrap();
    let root = new_root();
    let mut c = core(&root, &g);
    let target = first_shown(&e, &g);
    let old = c.export_state(&mut OsRng).unwrap();
    new_ping(&mut c, &g, &target);
    let new = c.export_state(&mut OsRng).unwrap();
    assert!(new.generation > old.generation);

    let mut relaunched = core(&root, &g);
    assert!(matches!(
        relaunched.import_state(&old.file, new.generation),
        Err(CoreError::Stale { .. })
    ));
    assert!(relaunched.pending_ops().is_empty());
    assert!(relaunched.visible_objects().is_empty());
    // The next file written is newer than either.
    assert!(relaunched.export_state(&mut OsRng).unwrap().generation > new.generation);
    let mut again = core(&root, &g);
    let report = again.import_state(&new.file, new.generation).unwrap();
    assert_eq!(report.generation, new.generation);
    assert_eq!(again.pending_ops().len(), 4);
}

#[test]
fn pending_ops_the_new_clearance_cannot_read_are_reported() {
    let e = enclosure();
    let osei = e.grant("k.osei").unwrap();
    let root = new_root();
    let mut c = core(&root, &osei);
    let target = first_shown(&e, &osei);
    new_ping(&mut c, &osei, &target);
    let file = c.export_state(&mut OsRng).unwrap().file;
    // The same persona, relaunched at a lower clearance (the dataset changed).
    let ks = SealedRootKeystore::open(&root, &SEALER).unwrap();
    let mut lower = ClientCore::open_scoped(
        provider(),
        ks,
        e.grant("k.rao").unwrap().clearance().clone(),
        SecurityFloor::NonExtractable,
        "dev-1",
        "k.osei",
    )
    .unwrap();
    let report = lower.import_state(&file, 0).unwrap();
    assert!(report.pending_dropped);
    assert!(lower.pending_ops().is_empty());
    // The clock still came back: it is kept at the lowest marking.
    let next = lower
        .edit_field(
            "note",
            "f",
            b"x".to_vec(),
            policy::Marking {
                level: policy::ClassificationLevel::UNCLASSIFIED,
                compartments: Default::default(),
            },
            &mut OsRng,
        )
        .unwrap();
    assert!(next.version.lamport > 4);
    // Read at the same clearance, nothing is dropped.
    let mut same = core(&root, &osei);
    assert!(!same.import_state(&file, 0).unwrap().pending_dropped);
}

#[test]
fn reserved_keys_cannot_be_written_from_outside() {
    let e = enclosure();
    let g = e.grant("k.rao").unwrap();
    let mut c = core(&new_root(), &g);
    let rows = e.fetch(&g, "midgard.audit").unwrap();
    for name in ["", "\0sys", "a/b", "a#b", "sys\0"] {
        assert!(
            matches!(
                c.cache_delivery(name, &rows, &mut OsRng),
                Err(CoreError::Reserved(_))
            ),
            "collection {name:?} accepted"
        );
    }
    assert!(PENDING_OBJECT.starts_with('\0'));
    assert!(matches!(
        c.edit_field(
            PENDING_OBJECT,
            "f",
            b"x".to_vec(),
            own_marking(&g),
            &mut OsRng
        ),
        Err(CoreError::Reserved(_))
    ));
    assert!(c.visible_objects().is_empty());
}

#[test]
fn a_lower_clearance_for_the_same_persona_drops_the_crdt_view_too() {
    let e = enclosure();
    let osei = e.grant("k.osei").unwrap();
    let rao = e.grant("k.rao").unwrap();
    let target = first_shown(&e, &osei);
    let mut c = core(&new_root(), &osei);
    let id = new_ping(&mut c, &osei, &target);
    let (c, err) = match c.rescope("k.osei", rao.clearance().clone()) {
        Err(pair) => pair,
        Ok(_) => panic!("lowered the clearance with pending ops"),
    };
    assert!(matches!(err, CoreError::PendingOps(4)));
    let mut c = c;
    c.drain_pending();
    let (c, _) = c
        .rescope("k.osei", rao.clearance().clone())
        .map_err(|(_, e)| e)
        .unwrap();
    assert_eq!(c.read_field(&id, action::VERB), None);
    assert!(c.visible_objects().is_empty());
    // Raising it again keeps what there is.
    let mut c = c;
    c.edit_field("n", "f", b"1".to_vec(), own_marking(&rao), &mut OsRng)
        .unwrap();
    let (c, dropped) = c
        .rescope("k.osei", osei.clearance().clone())
        .map_err(|(_, e)| e)
        .unwrap();
    assert_eq!(dropped, 0);
    assert_eq!(c.read_field("n", "f"), Some(b"1".to_vec()));
}
