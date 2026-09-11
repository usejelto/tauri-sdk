#!/usr/bin/env python3
"""C19: compare two clean local builds; produce packages and checksums, never publish."""
import hashlib
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import json
import tomllib

ROOT = Path(__file__).resolve().parent
CARGO = os.environ.get("CARGO", "cargo")
NPM = shutil.which("npm") or "npm"


def run(args, cwd, env=None):
    subprocess.run(args, cwd=cwd, env=env, check=True)


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    crate_version = tomllib.loads((ROOT / 'Cargo.toml').read_text())['package']['version']
    npm_version = json.loads((ROOT / 'package.json').read_text())['version']
    if crate_version != npm_version:
        raise SystemExit('Tauri Rust/npm versions must match')
    crate_name = f'tauri-plugin-jelto-{crate_version}.crate'
    npm_name = f'jelto-tauri-{npm_version}.tgz'
    outputs = []
    with tempfile.TemporaryDirectory(prefix="jelto-tauri-c19-") as scratch:
        for number in (1, 2):
            source = Path(scratch) / str(number)
            source.mkdir()
            for name in ("src", "permissions", "guest-js"):
                shutil.copytree(ROOT / name, source / name)
            for name in ("Cargo.toml", "Cargo.lock", "build.rs", "rust-toolchain.toml", "package.json", "package-lock.json", "tsconfig.json", "README.md", "LICENSE"):
                shutil.copy2(ROOT / name, source / name)
            # Build using exactly the already-installed lockfile toolchain, with clean outputs.
            if os.name == "nt":
                shutil.copytree(ROOT / "node_modules", source / "node_modules")
            else:
                (source / "node_modules").symlink_to(ROOT / "node_modules", target_is_directory=True)
            env = os.environ.copy()
            env["CARGO_TARGET_DIR"] = str(source / "target")
            env["RUSTFLAGS"] = " ".join(filter(None, [env.get("RUSTFLAGS", ""), f"--remap-path-prefix={source}=/jelto-tauri"]))
            run([CARGO, "build", "--release", "--locked", "--no-default-features", "--features", "conformance", "--bin", "conformance-host"], source, env)
            # Shipping Rust artifact is a source crate; validate it through cargo package.
            run([CARGO, "package", "--allow-dirty", "--locked", "--no-verify"], source, env)
            run([NPM, "pack", "--pack-destination", str(source)], source)
            suffix = ".exe" if os.name == "nt" else ""
            files = {
                crate_name: source / "target/package" / crate_name,
                npm_name: source / npm_name,
                f"conformance-host{suffix}": source / f"target/release/conformance-host{suffix}",
            }
            outputs.append(files)
        for name in outputs[0]:
            if digest(outputs[0][name]) != digest(outputs[1][name]):
                raise SystemExit(f"C19 failed: {name} differs between clean builds")
        destination = ROOT / "artifacts"
        destination.mkdir(exist_ok=True)
        for name, path in outputs[0].items():
            shutil.copy2(path, destination / name)
        (destination / "CHECKSUMS").write_text("".join(f"{digest(path)}  {name}\n" for name, path in sorted(outputs[0].items())))
        print(f"C19 passed: byte-identical artifacts in {destination}")


if __name__ == "__main__":
    main()
