# Run the Tauri 2 example

Install Rust 1.94.0, Node 22+ and the native Tauri prerequisites for your OS.
From the repository root, pack the bindings, install the example, and start it:

```sh
npm --prefix sdk/tauri ci
npm pack ./sdk/tauri --pack-destination sdk/tauri
node sdk/tauri/scripts/prepare-example.mjs
npm --prefix sdk/tauri/example ci
npm --prefix sdk/tauri/example run tauri dev
```

In another terminal, run the local mock ingest endpoint:

```sh
go -C contracts run ./spec/conformance/mockd -addr 127.0.0.1:8398
```

The defaults use this local endpoint and the conformance key. Check consent,
enable analytics, then try export, onboarding, install properties, reset and
disable. `JELTO_DEBUG=1` on the application process prints exact sent payloads
to stderr. Use the mock recording to verify `s=app`, `v=tauri/1.0.0` and the
registered identifier; no request should precede consent. For a real product,
replace the key, registered app identifier and endpoint, and allowlist `export`
with its `format` property under Settings → Events.

The Cargo dependency points to the local crate. The npm dependency points to
the locally packed tarball, testing the same bindings consumers install. The
preparation step updates only that tarball's integrity in the example lockfile;
registry dependencies retain their pinned versions and checksums. For the
complete CI build, run `make -C sdk/tauri example` from the repository root. The
capability permits only the local `main` window. The plugin is registered in
Rust; the frontend explicitly initializes it after consent. This example keeps
consent in memory for demonstration; your app should use its own consent UI and
persistence policy. Rust applications may also call `app.jelto().track(...)`
with `use tauri_plugin_jelto::JeltoExt`.
