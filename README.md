# Jelto for Tauri 2

Rust plugin `tauri-plugin-jelto` and TypeScript bindings `@jelto/tauri` for macOS,
Windows and Linux. `@jelto/tauri` is published on npm and `tauri-plugin-jelto` on
crates.io. See [the integration guide](https://jelto.io/docs/sdk/tauri) and
[the runnable example](https://github.com/usejelto/tauri-sdk/blob/main/example/README.md).

```sh
npm install @jelto/tauri
cd src-tauri
cargo add tauri-plugin-jelto
```

To build local packages instead, run `npm ci` and `make package` from this component's
source root. The installable npm and Rust packages are written to `artifacts/`;
`make example` prepares and builds a desktop app using the local plugin. See the
example README for its prerequisites and setup commands.

Register `.plugin(tauri_plugin_jelto::init())`, grant `jelto:default` to the local
window's capability, and initialize only after the app's consent decision:

```ts
import jelto from '@jelto/tauri'

await jelto.init('YOUR_PRODUCT_ID', 'desktop', undefined, 'new')
await jelto.track('export', { format: 'pdf' })
```

The fourth argument, `'new'`, is for an app that had no users before Jelto;
otherwise see the [existing-app guide](https://jelto.io/docs/start/existing-app).

Replace `YOUR_PRODUCT_ID` with your product ID (for example `prd_8f3kq2m9x1`) and
`desktop` with your app slug registered under **Settings → Installation → Apps**.
The optional third argument overrides the endpoint; `undefined` uses the default.

For an app with existing users, derive the fourth argument for each installation
from the app's saved first-launch or onboarding state, inspected before
overwriting it: `'existing'` when that state shows the installation predates
Jelto, `'new'` only when the host knows this is the installation's first launch
(an incomplete onboarding flag alone cannot establish it), and `'unknown'` when
unsure. Omitting it also sends unknown. Never hardcode one value for every
installation of an existing app. Retention, onboarding and license-conversion
reports count only installations marked `new`. Only this coarse value is sent,
never a date. Rust callers can use
`sdk.init_with_origin(key, None, None, origin).await` with a
`tauri_plugin_jelto::InstallOrigin` (`New`, `Existing` or `Unknown`);
the existing `init` method defaults to unknown.

The initial claim persists the classification, sends it only on `install` as
`props.install_origin`, and retains it across retries and launches regardless of
later init arguments. Legacy claims remain unknown. `setProps` cannot set this
reserved property. Reset creates an unknown claim; disable followed by init captures
the new explicit hint. See [adopting Jelto with existing users](https://jelto.io/docs/start/existing-app).

Seven asynchronous operations: `init`, `track`, `onboarding`, `setProps`,
`installId`, `reset`, `disable`. Rust exposes the same methods in snake_case
through `JeltoExt::jelto()`. Calls fail softly; identity reads return an empty
string when unavailable. The Rust worker owns all storage and network activity.
Registration creates no files or sockets. One product/app per process; multiple
windows share the same engine. Do not have multiple processes share its directory.

Initialization remembers the displayed app version. The first known version, including
state upgraded from older SDKs, establishes a baseline. Later changes automatically queue
`app_updated` with `from_version` and `to_version`, including downgrades, while preserving
the install ID and install count. Detection runs on every new launch regardless of the
daily heartbeat. Missing or overlong versions leave the baseline unchanged. Transitions
and their original metadata survive offline launches and retries; Reset starts a new
baseline and Disable wipes it. The server must support `app_updated` before adopting
this SDK. Failed transition persistence blocks delivery until its checkpoint succeeds;
the existing queue limits and final-response handling still apply.

`JELTO_DEBUG=1` prints payloads and diagnostics to stderr. `JELTO_ENDPOINT`
overrides the default `https://in.jelto.io/v1/e`; an explicit endpoint wins.
`JELTO_STATE_DIR` replaces the entire state directory. `JELTO_NOW`,
`JELTO_CLIENT_VERSION` and `JELTO_MOCK` are conformance overrides; leave them unset
in shipped apps. Storage failure falls back to memory; identity then cannot be
guaranteed across launches. Exit flushing is best effort and does not delay exit.

## Verify it works

1. Start the app with `JELTO_DEBUG=1` in its environment (for example `JELTO_DEBUG=1 npm run tauri dev`); payloads and diagnostics print to stderr. Leave it unset in shipped builds.
2. Let the app call `init` after its consent decision and keep it open for about 10 seconds.
3. In the Jelto dashboard, open **Settings → Installation → Apps**; your app shows **Receiving app activity**.

## Development and conformance

From the repository root:

```sh
make test
make conformance-twice
make package
make example
```

Rust 1.94.0 is pinned. `CARGO=/path/to/cargo` selects an installed toolchain.
`make test` includes real command ACL tests with Tauri's mock runtime and
C11 allocation/latency checks. `make package` performs two clean release
builds, compares the host binaries and installable packages, and writes
`artifacts/CHECKSUMS`. It publishes nothing. Native CI also builds the
example on macOS, Windows and Linux; headless conformance does not certify UI
runtime integration by itself.

Run development commands from this SDK directory. Set `JELTO_CONTRACTS_DIR` to
an extracted Jelto contracts **0.1.5** archive before running conformance. The
standalone Makefile owns Rust, TypeScript, packaging and host build commands.

## Repository CI and releases

The component-owned workflows become active when this directory is the
repository root. CI runs local package tests; release CI additionally requires
conformance twice and the configured contracts pin where applicable.
See [RELEASING.md](https://github.com/usejelto/tauri-sdk/blob/main/RELEASING.md) for initial publication, trusted publishing,
version tags, and retries. Publishing stays disabled until explicitly configured.

## Community and license

Questions, bug reports and documentation improvements are welcome. See
[Support](https://github.com/usejelto/tauri-sdk/blob/main/SUPPORT.md),
[Contributing](https://github.com/usejelto/tauri-sdk/blob/main/CONTRIBUTING.md),
[Code of Conduct](https://github.com/usejelto/tauri-sdk/blob/main/CODE_OF_CONDUCT.md), and
[Security policy](https://github.com/usejelto/tauri-sdk/blob/main/SECURITY.md).
Contact [taha@jelto.io](mailto:taha@jelto.io) for anything else.

Jelto-owned software and associated documentation use the [MIT license](LICENSE).
Third-party materials retain their own terms, including the Contributor Covenant
attribution. Jelto names, logos, mascots and original brand artwork are excluded
from the software license; no trademark rights are granted.

## Specification references

Source comments cite `spec/wire-v1.md` (the wire contract: envelope, fields, statuses,
retry rules) and `spec/sdk-conformance.md` (the behavioural contract, whose `C…` and `W…`
identifiers name conformance scenarios). Neither file ships in this repository: both live in
the public contracts repository at <https://github.com/usejelto/contracts/tree/main/spec>.
A comment that states a rule in words and then cites a section is pointing at the normative
text for that rule.
