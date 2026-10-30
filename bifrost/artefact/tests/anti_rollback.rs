//! Anti-rollback: the registry refuses a validly signed release older than one it has
//! already accepted for that id, and the same version with a different manifest; a
//! durable registry keeps its floor across a restart and refuses a store that disagrees
//! with it. [MG-SEC-14] [AC-B-1]
#![cfg(feature = "backend-fips")]

use std::path::PathBuf;

use attest::RollbackError;
use bifrost_artefact::{
    build_and_sign, generate_sbom, ArtefactRegistry, DurableArtefactRegistry, RegistryError,
    SbomComponent, SignedArtefact,
};
use pqc_agility::backend::fips::FipsSig;
use pqc_agility::{OsRng, SignatureScheme, SigningKey, VerifyingKey};

fn keys() -> (VerifyingKey, SigningKey) {
    FipsSig::new(bifrost_artefact::BIFROST_SIG_ALG)
        .keypair(&mut OsRng)
        .unwrap()
}

fn release(version: &str, payload: &[u8], sk: &SigningKey) -> SignedArtefact {
    let sbom = generate_sbom(
        "forge-pipeline",
        version,
        "service",
        vec![SbomComponent::new("forge-engine", "0.0.0")],
    );
    build_and_sign(
        "forge-pipeline",
        version,
        "service",
        payload.to_vec(),
        sbom,
        sk,
    )
}

fn scratch(tag: &str) -> PathBuf {
    let uniq = Box::into_raw(Box::new(0u8)) as usize;
    let dir =
        std::env::temp_dir().join(format!("anti-rollback-{tag}-{}-{uniq}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn older(e: RegistryError) -> bool {
    matches!(e, RegistryError::Rollback(RollbackError::Older { .. }))
}

#[test]
fn an_older_signed_release_is_refused_on_ingest() {
    let (vk, sk) = keys();
    let mut registry = ArtefactRegistry::new(vk);
    registry.ingest(release("1.4.0", b"v1.4.0", &sk)).unwrap();

    // Validly signed by the same release key, and still refused: it is older.
    let e = registry
        .ingest(release("1.3.9", b"v1.3.9 (vulnerable)", &sk))
        .unwrap_err();
    assert!(older(e.clone()), "{e}");
    assert!(registry.get("forge-pipeline", "1.3.9").is_none());

    // The same version with different signed content is equivocation.
    let e = registry
        .ingest(release("1.4.0", b"another 1.4.0", &sk))
        .unwrap_err();
    assert!(
        matches!(e, RegistryError::Rollback(RollbackError::Conflict { .. })),
        "{e}"
    );
    // The original 1.4.0 is untouched, and re-ingesting it is harmless.
    assert_eq!(
        registry.get("forge-pipeline", "1.4.0").unwrap().payload,
        b"v1.4.0"
    );
    registry.ingest(release("1.4.0", b"v1.4.0", &sk)).unwrap();

    // A pre-release of the accepted version is older than it.
    assert!(older(
        registry
            .ingest(release("1.4.0-rc.1", b"rc", &sk))
            .unwrap_err()
    ));
    // A newer release raises the floor; the accepted 1.4.0 stays deployable.
    registry.ingest(release("1.5.0", b"v1.5.0", &sk)).unwrap();
    assert!(registry
        .verify_for_deploy("forge-pipeline", "1.4.0")
        .is_ok());
    assert!(older(
        registry
            .ingest(release("1.4.1", b"v1.4.1", &sk))
            .unwrap_err()
    ));

    // A label that cannot be ordered is refused, not waved through.
    assert!(matches!(
        registry.ingest(release("latest", b"?", &sk)).unwrap_err(),
        RegistryError::Rollback(RollbackError::Unversioned { .. })
    ));
}

#[test]
fn a_tampered_older_release_fails_its_signature_before_the_floor_is_consulted() {
    let (vk, sk) = keys();
    let mut registry = ArtefactRegistry::new(vk);
    registry.ingest(release("1.4.0", b"v1.4.0", &sk)).unwrap();
    // A forged "2.0.0" must not raise the floor: the signature fails first.
    let mut forged = release("1.4.0", b"v1.4.0", &sk);
    forged.manifest.version = "2.0.0".into();
    assert!(matches!(
        registry.ingest(forged).unwrap_err(),
        RegistryError::Verify(_)
    ));
    registry.ingest(release("1.4.1", b"v1.4.1", &sk)).unwrap();
}

#[test]
fn a_durable_registry_keeps_its_floor_across_a_restart() {
    let dir = scratch("durable");
    let (vk, sk) = keys();
    {
        let mut registry = DurableArtefactRegistry::open(&dir, vk.clone()).unwrap();
        registry.ingest(release("1.3.0", b"v1.3.0", &sk)).unwrap();
        registry.ingest(release("1.4.0", b"v1.4.0", &sk)).unwrap();
    }
    // Restarted: both releases are back, and the floor still refuses an older one.
    let mut registry = DurableArtefactRegistry::open(&dir, vk.clone()).unwrap();
    assert_eq!(registry.len(), 2);
    assert!(registry
        .verify_for_deploy("forge-pipeline", "1.3.0")
        .is_ok());
    let e = registry
        .ingest(release("1.3.5", b"v1.3.5", &sk))
        .unwrap_err();
    assert!(older(e.clone()), "{e}");
    drop(registry);

    // Deleting the floor file does not reset it to nothing: the registry adopts the
    // highest release it holds.
    std::fs::remove_file(dir.join("release.floor")).unwrap();
    let mut registry = DurableArtefactRegistry::open(&dir, vk.clone()).unwrap();
    assert!(older(
        registry
            .ingest(release("1.3.5", b"v1.3.5", &sk))
            .unwrap_err()
    ));
    drop(registry);

    // A store that disagrees with its floor — a 1.4.0 that is not the recorded one —
    // fails closed on boot.
    let other = {
        let scratch_dir = scratch("other");
        let mut r = DurableArtefactRegistry::open(&scratch_dir, vk.clone()).unwrap();
        r.ingest(release("1.4.0", b"a different 1.4.0", &sk))
            .unwrap();
        drop(r);
        let blob = std::fs::read_dir(&scratch_dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .find(|p| p.extension().is_some_and(|e| e == "artefact"))
            .unwrap();
        let bytes = std::fs::read(&blob).unwrap();
        let _ = std::fs::remove_dir_all(&scratch_dir);
        (blob.file_name().unwrap().to_owned(), bytes)
    };
    std::fs::write(dir.join(other.0), other.1).unwrap();
    assert!(matches!(
        DurableArtefactRegistry::open(&dir, vk).err(),
        Some(RegistryError::Rollback(RollbackError::Conflict { .. }))
    ));
    let _ = std::fs::remove_dir_all(&dir);
}
