# Contributing to Jelto for Tauri 2

Bug reports, documentation corrections, examples and focused code changes are
welcome. Participants follow our [Code of Conduct](CODE_OF_CONDUCT.md).

## Start a contribution

Search [existing issues](https://github.com/usejelto/tauri-sdk/issues) before opening one. Use a bug report for
reproducible failures, a feature request to explain a use case, or a blank issue
for questions and documentation feedback. Discuss substantial features and
breaking changes before implementing them. Small fixes can go straight to a PR.

Send security reports privately as described in [SECURITY.md](SECURITY.md).
For installation and account help, see [SUPPORT.md](SUPPORT.md).

## Local development

Rust 1.94.0 (see `rust-toolchain.toml`), Node.js 24, npm, Make, Python 3.11 or later, and the Go toolchain required by the contracts archive. Install the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for your OS to run the desktop tests and example. Set `CARGO=/absolute/path/to/cargo` if needed.

Fork and clone this repository, create a branch from `main`, and run commands
from this component's root. The standalone checkout contains its build inputs;
you do not need the private Jelto backend.

Shared conformance needs a verified Jelto contracts **0.1.8** archive. Download
it and its checksum from the [contracts releases](https://github.com/usejelto/contracts/releases)
when published, then install it into an empty directory:

```sh
python3 vendor/test-tools/install.py /absolute/path/jelto-contracts-0.1.8.zip SHA256 /absolute/path/contracts --version 0.1.8
export JELTO_CONTRACTS_DIR=/absolute/path/contracts
```

Replace `SHA256` with the checksum from that release. Use the Go version in the
archive's `go.mod`. Before the first public release, obtain the reviewed archive
from the maintainer. Monorepo contributors may explicitly point
`JELTO_CONTRACTS_DIR` at the local contracts root. Builds must not discover a
sibling checkout implicitly.

```sh
npm ci
make test
make conformance-twice
make package
make example
```

Run the smallest relevant checks while iterating, then the affected gates above.
Documentation-only changes need link and example review; SDK behavior or wire
changes require the appropriate tests and two consecutive conformance passes
before certification. Package changes must also pass packaging checks.

Keep Rust and TypeScript regression tests beside the relevant implementation.
`make test` covers command permissions and C11 runtime budgets; `make package`
checks C19 reproducibility. Run `make example` for desktop integration changes.
Use the contracts wire and conformance specifications for expected behavior.
Report the tested OS and toolchain: headless conformance does not certify desktop
UI integration on every supported platform.

## Checks and exceptions

`make check` is the whole gate, and the command CI runs on Linux. It runs `test`, `build`, `package` and `security`; CI also builds the
native example on every platform. `make hooks`
enables two Git hooks once per clone: `commit-msg` rejects a subject that is
not a Conventional Commit, and `pre-commit` runs gitleaks over the staged
changes. Do not bypass them with `--no-verify`.

The security targets run separate programs, not project dependencies.
`vendor/quality/tools.json` pins each one's exact version and SHA-256; the first run
downloads and verifies it into `~/.cache/jelto-tools` (`JELTO_TOOLS_DIR`
overrides the location), and `make tools` installs them all ahead of time.
`vendor/quality/` is a checksummed snapshot of the contracts repository's
`tools/quality/`; change it there, not here.

| Target | Tool | What it checks |
|---|---|---|
| `make secrets` | gitleaks | the whole Git history for credentials |
| `make workflow-audit` | actionlint, zizmor | workflow syntax, script injection, permissions and pinning |
| `make osv` | OSV-Scanner | `Cargo.lock`, `package-lock.json` and the example's lockfiles against the OSV database, downloaded and matched locally |
| `make npm-audit` | npm audit | `package-lock.json` against the npm advisory database |
| `make cargo-audit` | cargo-audit | `Cargo.lock` against the RustSec advisory database |

When a check reports something, fix a real problem in the same change. A
deliberate exception, or a finding the tool got wrong, is marked at the one
place it applies, with the reason beside it:

- gitleaks: a `.gitleaks.toml` allowlist entry naming the one literal and why;
- zizmor: `# zizmor: ignore[<audit>]` on the line, with the reason above it;
- OSV-Scanner: an `[[IgnoredVulns]]` entry in `osv-scanner.toml` with a
  `reason` and an `ignoreUntil` date.

Never disable a rule, a file or a directory wholesale, and never relax a
threshold to make a check pass.

## Review and acceptance

Keep changes focused and match the surrounding style.
Format Rust changes with `cargo fmt`; TypeScript uses strict types and two-space indentation.
Add regression coverage for behavior changes. Do not update expected values just to match a
failing implementation, and do not edit generated or vendored inputs by hand.

Use a scoped Conventional Commit title, such as `fix(tauri): clarify retry handling` or `docs(tauri): explain local installation`. Describe the problem,
the resulting behavior, relevant issue/specification, compatibility impact and
exact verification commands with results. Include screenshots for visible UI
changes. State when a check was not run and why. A separate specification change
should precede its implementation; fixture expectation changes need a separate
commit explaining their derivation.

Use synthetic data in examples and reports. Never include credentials, customer
payloads or IP addresses in public logs. Contributions must preserve Jelto's
privacy boundaries: no IP storage or logging, and no row-level identifier that
joins a website visitor to an app install.

Taha Bozdemir reviews scope, correctness, tests and compatibility, and decides
whether to merge. Review is best effort with no guaranteed turnaround. Maintainers
may request changes or decline work outside the component's purpose; explain
tradeoffs in the issue or PR so future contributors can follow the decision.

## Licensing and releases

Submit only work you have the right to contribute, under this repository's
[MIT license](LICENSE). Preserve third-party notices and the Contributor Covenant
attribution. Jelto names, logos, mascots and original brand artwork are excluded
from the software license; no trademark rights are granted. No separate CLA or
DCO sign-off is required.

Maintainers publish releases using [RELEASING.md](RELEASING.md). Local packaging
does not publish, and a successful test run is not a release announcement.
