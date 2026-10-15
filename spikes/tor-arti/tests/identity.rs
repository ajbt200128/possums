#![cfg(feature = "arti")]

use possums_tor_arti_spike::{embedding::require_identity, Failure};
use std::{fs, os::unix::fs::PermissionsExt, path::Path};
use tor_hscrypto::pk::{HsId, HsIdKey, HsIdKeypair};
use tor_hsservice::{HsIdKeypairSpecifier, HsIdPublicKeySpecifier, HsNickname};
use tor_keymgr::{ArtiNativeKeystore, KeyMgr, KeyMgrBuilder, KeystoreSelector};

fn scratch() -> tempfile::TempDir {
    tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap()
}

fn open(path: &Path) -> KeyMgr {
    let store = ArtiNativeKeystore::from_path_and_mistrust(path, &fs_mistrust::Mistrust::default())
        .unwrap();
    KeyMgrBuilder::default()
        .primary_store(Box::new(store))
        .build()
        .unwrap()
}

// Explicit synthetic provisioning, never called by the read-only startup gate.
fn provision(manager: &KeyMgr, name: &HsNickname) -> HsId {
    let pair = manager
        .generate::<HsIdKeypair>(
            &HsIdKeypairSpecifier::new(name.clone()),
            KeystoreSelector::Primary,
            &mut tor_llcrypto::rng::CautiousRng,
            false,
        )
        .unwrap();
    // KeyMgr derives the public identity from the pair; no redundant public file.
    HsIdKey::from(&pair).id()
}

// Copies only between RAII scratch directories, never to repository artifacts.
fn copy_state(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).unwrap();
    fs::set_permissions(destination, fs::Permissions::from_mode(0o700)).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let to = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_state(&entry.path(), &to);
        } else {
            fs::copy(entry.path(), to).unwrap();
        }
    }
}

#[test]
fn native_key_manager_identity_survives_reopen_and_offline_restore() {
    let original = scratch();
    let restored = scratch();
    let name: HsNickname = "synthetic-spike".parse().unwrap();
    let approved = {
        let manager = open(original.path());
        let approved = provision(&manager, &name);
        assert!(require_identity(&manager, &name, &approved).is_ok());
        approved
    };
    {
        let reopened = open(original.path());
        assert!(require_identity(&reopened, &name, &approved).is_ok());
    }
    // No publisher exists, and the original manager is dropped before restore.
    copy_state(original.path(), restored.path());
    let manager = open(restored.path());
    assert!(require_identity(&manager, &name, &approved).is_ok());
}

#[test]
fn absent_pair_or_wrong_approved_identity_never_generates() {
    let scratch = scratch();
    let manager = open(scratch.path());
    let name: HsNickname = "synthetic-spike".parse().unwrap();
    let other: HsNickname = "synthetic-other".parse().unwrap();
    let approved = provision(&manager, &name);
    let wrong = provision(&manager, &other);
    assert_eq!(
        require_identity(&manager, &name, &wrong),
        Err(Failure::IdentityMismatch)
    );
    let public = manager
        .get::<HsIdKey>(&HsIdPublicKeySpecifier::new(name.clone()))
        .unwrap()
        .unwrap();
    manager
        .remove::<HsIdKeypair>(
            &HsIdKeypairSpecifier::new(name.clone()),
            KeystoreSelector::Primary,
        )
        .unwrap();
    // Public-only restore is not enough to publish; the gate must not create a pair.
    manager
        .insert(
            public,
            &HsIdPublicKeySpecifier::new(name.clone()),
            KeystoreSelector::Primary,
            false,
        )
        .unwrap();
    assert_eq!(
        require_identity(&manager, &name, &approved),
        Err(Failure::IdentityMissing)
    );
    assert!(manager
        .get::<HsIdKeypair>(&HsIdKeypairSpecifier::new(name.clone()))
        .unwrap()
        .is_none());
}

#[test]
fn stored_public_key_cannot_disagree_with_private_identity() {
    let scratch = scratch();
    let manager = open(scratch.path());
    let name: HsNickname = "synthetic-spike".parse().unwrap();
    let other: HsNickname = "synthetic-other".parse().unwrap();
    let approved = provision(&manager, &name);
    provision(&manager, &other);
    let wrong = manager
        .get::<HsIdKey>(&HsIdPublicKeySpecifier::new(other))
        .unwrap()
        .unwrap();
    manager
        .insert(
            wrong,
            &HsIdPublicKeySpecifier::new(name.clone()),
            KeystoreSelector::Primary,
            true,
        )
        .unwrap();
    assert_eq!(
        require_identity(&manager, &name, &approved),
        Err(Failure::IdentityMismatch)
    );
}
