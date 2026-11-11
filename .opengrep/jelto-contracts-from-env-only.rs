use std::env;
use std::path::{Path, PathBuf};

fn contracts_dir() -> PathBuf {
    // ruleid: jelto-contracts-from-env-only
    let sibling = PathBuf::from("../contracts");
    // ruleid: jelto-contracts-from-env-only
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../jelto-contracts/spec");
    // ruleid: jelto-contracts-from-env-only
    let joined = Path::new("..").join("contracts");
    // ruleid: jelto-contracts-from-env-only
    let windows = PathBuf::from("..\\contracts");
    let _ = (sibling, manifest, joined, windows);

    // ok: jelto-contracts-from-env-only
    let from_env = env::var_os("JELTO_CONTRACTS_DIR").map(PathBuf::from);
    // ok: jelto-contracts-from-env-only
    let local = Path::new(".contracts").join("spec");
    // ok: jelto-contracts-from-env-only
    let vendored = Path::new("../vendor/quality/quality.mk");
    let _ = (local, vendored);
    from_env.unwrap_or_default()
}
