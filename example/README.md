# Run the Tauri 2 example

Install Rust 1.94.0, Node 22+ and the native Tauri prerequisites for your OS.
From the repository root, pack the bindings, install the example, and start it:

```sh
npm ci
npm pack --pack-destination .
node scripts/prepare-example.mjs
npm --prefix example ci
npm --prefix example run tauri dev
```

In another terminal, run the local mock ingest endpoint from an extracted Jelto
contracts archive (the same `JELTO_CONTRACTS_DIR` used for conformance):

```sh
go -C "$JELTO_CONTRACTS_DIR" run ./spec/conformance/mockd -addr 127.0.0.1:8398
```

The form's defaults are for this mock only: the local endpoint and a
conformance test product ID. Check consent, enable analytics, then try export,
onboarding, install properties, reset and disable. `JELTO_DEBUG=1` on the
application process prints exact sent payloads to stderr. Use the mock recording
to verify `s=app`, a `v` of `tauri/` followed by the plugin version, and the
app slug; no request should precede consent.

To send to your own Jelto product instead, enter your product ID (for example
`prd_8f3kq2m9x1`) and an app slug registered under **Settings → Installation →
Apps**, and clear the endpoint field so the plugin uses its default ingest URL.
Custom events such as `export` and their properties are discovered when Jelto
first receives them; nothing needs to be registered first.

The Cargo dependency points to the local crate. The npm dependency points to
the locally packed tarball, testing the same bindings consumers install. The
preparation step updates only that tarball's integrity in the example lockfile;
registry dependencies retain their pinned versions and checksums. For the
complete CI build, run `make example` from the repository root. The
capability permits only the local `main` window. The plugin is registered in
Rust; the frontend explicitly initializes it after consent. This example keeps
consent in memory for demonstration; your app should use its own consent UI and
persistence policy. Rust applications may also call `app.jelto().track(...)`
with `use tauri_plugin_jelto::JeltoExt`.
