# Development, platforms and distribution

## Proposed repository

The following is a target structure; only documentation exists today. Start with three implementation crates and split further only when a boundary has independent consumers.

```text
Cargo.toml / Cargo.lock / rust-toolchain.toml
crates/
  app/src/{main.rs,session/,ui/,render/,config/,panels/}
  terminal-core/src/{lib.rs,adapter.rs,input.rs,snapshot.rs}
  platform/src/{lib.rs,pty.rs,paths.rs,unix.rs,windows.rs,macos.rs}
assets/{themes/,icons/,licenses/}
tests/{fixtures/,integration/,desktop/}
benches/
fuzz/
packaging/{windows/,macos/,linux/}
scripts/
.github/workflows/
docs/
  IMPLEMENTATION_PLAN.md / TECHNICAL_SPEC.md / DECISIONS.md
  TERMINAL.md / CONFIGURATION.md / SECURITY_AND_PROVENANCE.md
  TESTING.md / DEVELOPMENT.md
```

Future `docs/adr/`, `docs/provenance.csv`, `benchmarks/baseline.md`, `THIRD_PARTY_NOTICES`, `SECURITY.md` and license texts become required before distribution. A Cargo workspace and CI are M1 deliverables, not implied to exist now. Runtime behavior does not depend on OMX or any coding-agent tooling.

## Cross-platform strategy

| Area | Shared policy | Platform implementation / validation |
|---|---|---|
| Window/input | Typed actions, focus, IME contract | GUI/winit integration; macOS menu/Option/Command, Windows AltGr, Linux Wayland and X11 |
| PTY | Spawn/write/resize/close state machine | Unix PTY/process group; Windows ConPTY plus owned lifecycle handles; test descendant semantics |
| Rendering | Shared shader/text/damage path | Metal macOS, DX12 Windows, Vulkan Linux initially; enumerate actual adapter support |
| Paths/config | Precedence and migration | Native config/cache directories and atomic replacement |
| Clipboard/links | Explicit action and scheme policy | Framework/OS APIs; Wayland constraints, X11 selection optional and off by default |
| Fonts/DPI | Cell metrics and fallback policy | OS font discovery, per-monitor scale, missing-font handling |
| Metrics | CPU/memory schema and sampling | sysinfo or narrow OS APIs; no shelling out to `top` or `ps` |
| Lifecycle | Visible exit/error states | Suspend/resume, termination, signal and handle behavior |
| Accessibility | Focus, text, caret/selection semantics | Native accessibility bridge; prove availability per target |

Proposed initial Tier 1 artifacts: Windows 11 x86_64, macOS 14+ arm64, Ubuntu 24.04 x86_64 (Wayland and X11). These are intended test baselines, not a verified or universal compatibility statement. Reconfirm vendor support and the selected crate/toolchain minimums during M0. Support OS families from MVP; exact distribution/architecture promises follow hardware evidence. Candidate Tier 2: Intel macOS, Windows arm64, Linux arm64 and additional distributions; no release claim until native testing and artifacts exist. Windows 10/older macOS are not implied by underlying API availability. Document GPU/driver minimums from the chosen wgpu release and real adapters; no blanket promise that every integrated GPU works.

Platform-specific code is isolated, but do not hide real differences behind a misleading universal feature flag. Capability queries report IME/accessibility, supported backend and clipboard limitations to the app. Errors have safe user messages and technical codes. No Linux shell-command probes or hardcoded font paths.

## Build and contribution workflow

M0 spikes may have temporary manifests; M1 establishes the production workspace after the stack passes. Pin a stable Rust toolchain with rustfmt/clippy, set edition/MSRV, commit Cargo.lock, and document necessary native system libraries for Linux. Use platform-native runners for packages; cross-compilation cannot validate runtime behavior. No mandatory Node, browser runtime, Python service or external daemon at runtime.

After implementation exists, baseline commands (use the workspace-required RTK prefix here) are:

```sh
rtk cargo fmt --all -- --check
rtk cargo clippy --workspace --all-targets --locked -- -D warnings
rtk cargo test --workspace --locked
rtk cargo build --workspace --release --locked
```

Add explicit supported feature-combination jobs rather than blindly enabling mutually exclusive backend features with `--all-features`. Run license/advisory/static dependency checks using reviewed cargo-deny/cargo-audit policies, shader validation, and platform packaging checks. Only install tooling as part of an authorized implementation task. Miri/sanitizers are targeted to compatible headless/unsafe boundaries, not used as proof of driver safety.

Small PRs should include motivation, relevant ADR, scope, provenance declaration, tests actually run and known gaps. Require review of new dependencies, unsafe code, host-effect policies and protocol claims. Commit messages follow Lore: intent first, rationale, useful native trailers (`Constraint`, `Rejected`, `Confidence`, `Scope-risk`, `Tested`, `Not-tested`). Keep unrelated reformatting out of behavior changes. Add regression tests for bugs and benchmark before optimization. Changes to public config schema require migration fixtures and release notes.

CI tiers: each PR runs formatting, lint, headless unit/integration, supported-platform compile, dependency policy and docs/config validation; nightly runs fuzz smoke, native PTY/GPU tests and benchmark hosts; release runs installed signed-artifact desktop tests. Cache by lockfile/toolchain/target and never treat cache success as a substitute for a clean release build. Protect signing jobs from untrusted PR code; pin workflow actions to reviewed revisions.

## Packaging and release channels

| Platform | MVP artifact | Production additions and verification |
|---|---|---|
| Windows | Portable ZIP with executable/notices | Signed per-user installer (MSI or MSIX chosen in M4 ADR); verify Authenticode, no admin requirement by default, shortcuts/uninstall/upgrade |
| macOS | `.app` in DMG or ZIP for internal preview | Developer ID signing and notarization, stapling, Gatekeeper test on clean machine; minimal entitlements; verify bundled fonts/resources |
| Linux | Tarball and one `.deb` for baseline distro | Desktop entry/icon, dependency metadata and uninstall checks; additional formats only with tested host-shell access |

Unsigned MVP builds are explicitly internal/developer previews; public user-facing beta follows applicable signing/notarization gates. Package only app assets and notices, not user shells. Target system dependencies deliberately, document the Linux glibc baseline, and build on the oldest supported distribution. AppImage improves portability but adds integration testing; Flatpak sandbox/host-shell spawning requires a separate security and UX design and is not a default terminal package. Store distribution has sandbox/capability trade-offs and is deferred.

Release checklist: freeze tested revision and lockfile; clean builds per target; run correctness/security/performance gates; generate SBOM/notices/checksums; sign; verify signatures/notarization on the final distributed files; install and exercise core workflows on clean hosts; publish versioned release notes and limitations. Archive toolchain/dependency inputs and build attestations. Aim for reproducible inputs and investigate binary differences; do not claim bit-for-bit reproducible signing/notarization output.

Use pre-1.0 releases while schemas and interfaces evolve, explicit migration notes for every schema change, and stable semantic-versioning policy at 1.0. Initial updates are manual: download a signed package from the project release channel, verify, replace app, preserve config. Test N−1→N migration with backups and rollback instructions: old binaries use the backed-up old config, never blindly parse a newer schema. Uninstall preserves user config by default and offers documented explicit removal; never delete shell profiles, home directories, or terminal-created files.

Before production, define supported release window, vulnerability contact, release owner/backup and rollback authority. Start with one stable channel and one preview channel; automated updater, crash-upload services and update polling need separate decisions and are not silently introduced.

Distribution reference: follow [Apple's notarization documentation](https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution) when implementing the macOS release pipeline; revalidate service/tool requirements at release time.
