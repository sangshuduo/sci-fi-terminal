# Independent terminal: implementation plan and product requirements

Status: proposed planning baseline; architectural recommendations require M0 validation.
Date: 2026-09-22. Project and repository name: `sci-fi-terminal`.
Scope of this delivery: specifications and supporting documentation, not an implemented application.

## Purpose and independence boundary

Build a responsive everyday terminal with an optional original science-fiction-inspired workspace: useful system context, configurable panels, restrained animation, and multiple themes. A focused terminal remains the default experience. The application is Rust-first, uses native windows and GPU rendering, and shares its core across Windows, macOS, and Linux.

Do not read, copy, translate, port, trace, or adapt eDEX-UI source, implementation details, themes, assets, layouts, or UI components. Its general concept is the only inspiration. Design project behavior from this specification, public terminal protocol documentation, user research, and independently licensed dependencies. Maintain provenance for every dependency and asset. A dependency is permissible only after its selected version, provenance, license, and transitive obligations are recorded. This policy is an engineering provenance control, not a claim of legal clearance.

## Principles and decision drivers

Principles:

1. Terminal correctness and input responsiveness precede decorative effects.
2. Preserve user control: readable defaults, keyboard access, reversible settings, no surprise shell execution.
3. Bound resource use: event-driven redraw, finite scrollback and queues, visible resource limits.
4. Keep platform concerns behind small interfaces and retain platform-native behavior where it matters.
5. Add extension seams when there is a concrete consumer; avoid an initial plugin runtime or generalized UI engine.

The top three architectural drivers, in order, are **terminal correctness and latency**, **cross-platform maintainability**, and **GPU visual flexibility within measured efficiency budgets**. A renderer with spectacular effects but broken IME or high idle power does not meet these drivers.

## Alternatives and provisional recommendation

| Option | Advantages | Costs and risks | Decision |
| --- | --- | --- | --- |
| iced application shell with wgpu custom terminal widget | Rust-native declarative application state; reusable settings controls; shared GPU renderer; less application infrastructure to invent | Custom primitive integration and dependency-version coupling; terminal accessibility and IME still require explicit work; toolkit upgrades may affect rendering APIs | Recommended provisionally, subject to M0 proving terminal integration, text, input, and idle behavior |
| egui with its wgpu integration | Fast composition of settings and diagnostic panels; simple immediate-mode model; custom paint integration | Complex focus and terminal input arbitration need care; verify accessibility and IME on each OS; redraw discipline must be demonstrated rather than assumed | Credible fallback if iced integration fails; compare using the same test fixture |
| winit + wgpu with entirely project-owned UI | Direct event/render scheduling, strong control over batching and effects | Largest accessibility, text editing, focus, layout, settings, and maintenance burden; longer time to useful MVP | Reserve for evidence that both toolkit options prevent essential terminal requirements |

Native widget wrappers are viable for conventional desktop chrome but complicate a unified stylized GPU composition. Browser/Electron architectures do not satisfy the requirement. A game engine introduces systems and runtime scope that this terminal does not need.

Use `alacritty_terminal` provisionally behind a project-owned adapter, and `portable-pty` behind a session transport abstraction. These are separately licensed dependencies, not an eDEX-UI implementation foundation. A parser-only library plus an original terminal grid increases correctness and compatibility work considerably; a different mature terminal engine may offer stronger features but also a larger integration surface. M0 must inspect public APIs, licensing, release health, and conformance behavior before choosing exact versions. Do not promise a fully compatible terminal merely because an engine has been selected.

Use one compatible graphics stack resolved by the selected GUI release; do not independently select incompatible wgpu versions. Prefer the toolkit's text infrastructure when it can satisfy terminal cell shaping and clipping. Introduce a separate shaping/atlas dependency only when the text spike demonstrates a need.

## Product scope and acceptance criteria

### Working MVP

The MVP is an installable local terminal application on all three target OS families. It includes:

- One native window with tabs, two-way split panes, deterministic focus navigation, and visible session exit/error state.
- Local interactive shells through real PTYs, resize propagation, UTF-8, ANSI colors, alternate screen, cursor modes, selection, copy/paste, bounded scrollback, and literal-text search.
- A documented tested compatibility subset, demonstrated with shell editing, `less`, `vim` or `nvim`, `tmux` on Unix, and a PowerShell interactive workflow on Windows. Unsupported sequences are safely ignored or explicitly reported, never interpreted as application commands.
- Three original built-in themes plus a high-contrast preset; user-authored declarative themes; adjustable font family/size, spacing, cursor, palette, and reduced-motion behavior.
- Discoverable settings for the common appearance, terminal, and layout controls; TOML configuration for advanced options; diagnostics with a file/field location for invalid settings; reset to defaults.
- A command palette, editable keybindings with conflict diagnostics, configurable startup shell/profile, and saved layout geometry without restoring commands or terminal transcript by default.
- One optional local CPU/memory panel, sampled at a low fixed rate only when relevant; no external network service, daemon, or analytics requirement.
- Keyboard-only operation of primary workflows, a bounded accessible terminal text/caret representation, and a documented screen-reader/IME status matrix. Any accessibility gap is prominently recorded for MVP and blocks production release if it prevents core use.
- Subtle optional GPU visual treatment, with effects disabled in the baseline performance scenario and reduced motion respected.

MVP excludes remote session management, SSH credential storage, background session daemons, arbitrary native/WASM plugins, terminal image protocols, shell command replay, automatic updates, mobile/web editions, animated 3D scenes, and a visual layout editor. Ordinary `ssh` run by the user's shell still works as terminal traffic.

### Measurable gates

All numbers are proposed budgets, not observed results. M0 selects and records actual reference machines: a supported integrated-GPU Windows laptop, Apple Silicon Mac, and integrated-GPU Linux machine. Report OS, graphics backend, resolution, refresh rate, build profile, font, shell, and sample counts. Use release builds, effects off, 1920×1080, one 120×40 terminal, and default bounded scrollback unless specified. Distinguish application-ready from shell-prompt-ready timing.

| Metric | MVP acceptance target | Production target |
| --- | --- | --- |
| Warm launch to editable terminal surface | p95 ≤ 750 ms over 30 launches | p95 ≤ 500 ms over 50 launches |
| Cold launch after documented cold-start procedure | p95 ≤ 1.5 s | p95 ≤ 1.0 s |
| Input event to submitted frame under normal shell load | p95 ≤ 25 ms, p99 ≤ 50 ms | p95 ≤ 16.7 ms, p99 ≤ 33 ms; separately measure visible latency where instrumentation allows |
| Frame work during active scrolling on 60 Hz reference display | p95 ≤ 16.7 ms CPU+GPU critical path | p99 ≤ 16.7 ms; no sustained missed-frame bursts |
| Idle CPU, 60-second sample, no terminal output | ≤ 1% of one logical core; no continuous redraw when cursor/effects inactive | ≤ 0.5%; background/minimized decoration work suspended |
| Idle application memory excluding child shell | ≤ 200 MiB process footprint; report OS measurement definition | ≤ 150 MiB on each reference host; report GPU allocations separately |
| Sustained local output stress | Consume 10 MiB/s for 60 s with responsive input, finite memory, and no byte loss before parsing | 10 MiB/s aggregate for 30 min across eight active sessions; bounded input latency and no monotonic leak |
| GPU resource accounting | ≤128 MiB app-owned GPU resources at 1080p/one pane; no unbounded growth across 100 resizes | Stable settled allocation after stress; ≤128 MiB app-owned GPU resources at 1080p/one pane, ≤256 MiB at 4K/four panes |

Performance targets may change only through a documented ADR with measurements, user impact, and an explicit trade-off. Do not silently redefine workloads to pass a gate. The eight-session production workload is separate from the one-session idle memory target.

MVP also requires passing platform PTY lifecycle tests, parser/adapter fixture tests, configuration migration tests, and install/uninstall smoke tests. A recoverable GPU surface error must not terminate shell sessions. Unsupported hardware receives an actionable failure message; software rendering is not an MVP guarantee.

Production requires the MVP gates plus a published OS/architecture support matrix, security review of escape-sequence side effects, signed platform artifacts where applicable, reproducible dependency resolution, notices/SBOM, tested upgrades and rollback guidance, screen-reader coverage of core workflows, multi-language IME validation, fuzzing history without unresolved crashes, and no unresolved release-blocking correctness or data-loss issue. It also requires a documented support/vulnerability-reporting process and a release checklist exercised by someone other than the implementer.

## Implementation milestones

Estimates are engineering person-weeks, assuming experienced Rust developers and access to all three operating systems. They exclude signing-account procurement and external security review lead time. The schedule is not a commitment. Two engineers cannot halve all durations because renderer, terminal, and integration decisions are sequential.

| Milestone | Deliverables and exit evidence | Owner role | Estimate | Depends on |
| --- | --- | --- | --- | --- |
| M0: feasibility and decisions | Three-platform native shell/PTY/render spike; iced custom terminal primitive; IME, CJK/emoji, DPI, accessibility, resize/exit/cancel, GPU recovery, idle-power measurements; exact dependency/license matrix; ADR decision | Architecture owner with terminal/render engineer | 2–3 weeks | Planning baseline |
| M1: runnable foundation | Cargo workspace, shared domain types, platform adapters, native window, configuration diagnostics, CI on all OS families, provenance ledger | Application/platform engineer | 1–2 weeks | M0 stack decision |
| M2: reliable terminal vertical slice | PTY lifecycle, engine adapter, bounded transport, snapshots/damage, GPU terminal rendering, selection/paste/input, compatibility fixtures; basic shell usable daily | Terminal/render engineer | 4–6 weeks | M1 |
| M3: usable workspace | Tabs/split focus, palette, search, settings, original themes, optional system panel, keyboard shortcuts, saved layout; usability and resource checks | UI/application engineer | 3–4 weeks | M2; nonterminal UI can start against a mock adapter |
| M4: MVP validation and packages | Platform test matrix, benchmark report, installation artifacts, notices, documented limitations, first-user walkthrough; all MVP gates met | Release/test owner with implementation owners | 2–3 weeks | M3 |
| M5: beta hardening | Accessibility and IME completion, migration/upgrade cases, fuzzing, prolonged output/GPU recovery soak, external user feedback, performance regressions fixed | All owners; independent reviewer | 4–6 weeks | M4 |
| M6: production release | Security/release review, signing/notarization, SBOM, artifact verification, support policy, rollback/runbook exercise; production gates met | Release owner and independent reviewer | 2–3 weeks | M5 |

Planning range: 12–18 person-weeks through MVP and 18–27 through production, before contingency. Reserve roughly 25% contingency for terminal/platform integration and accessibility. One senior developer should plan approximately 4–6 calendar months for MVP including contingency; two developers can overlap platform, UI, and tests after the vertical-slice contracts stabilize. Re-estimate after M0 with measured unknowns.

### First implementation tickets after M0 approval

1. Record stack versions, supported targets, MSRV, license notices, and benchmark machines in an ADR.
2. Create the minimal workspace and CI with formatting, clippy, unit tests, and platform build jobs.
3. Define typed session IDs, ordered input/resize commands, session states, and renderer-facing immutable viewport snapshots.
4. Implement one PTY session with deterministic close/reap behavior and a fake transport for tests.
5. Wire terminal engine output to a GPU viewport; validate character cell geometry, clipping, and DPI transitions before visual effects.
6. Add clipboard/paste controls, IME composition routing, selections, and keybinding arbitration with regression fixtures.
7. Capture the first performance baseline before adding panels, split panes, or effects.

## Dependencies and decision records

Record these ADRs before their implementation boundary is crossed:

- ADR-001 through ADR-004 are proposed in [technology decisions](DECISIONS.md): GUI, terminal/PTY, rendering/text, configuration/extensions.
- Before M1, add accepted evidence for those decisions plus a threading/queue/shutdown ADR.
- Before M4, record platform support, packaging/signing, escape-sequence security and data-retention decisions.
- Before third-party executable extensions, supersede ADR-004 with the chosen protocol and threat model.

An ADR is accepted after review; its presence does not imply that its proposed technology has been verified. Dependencies remain proposals until added by an implementation task and resolved into a reviewed lockfile.

## Risks and responses

| Risk | Early signal | Response and gate |
| --- | --- | --- |
| iced cannot integrate terminal GPU/input/accessibility adequately | M0 primitive duplication, inaccessible text, broken composition, or idle repaint loop | Time-box to M0; compare egui on identical fixture; choose bespoke stack only with explicit schedule revision |
| Terminal engine embeds unsuitable assumptions or unstable APIs | Adapter requires exposing engine internals throughout UI | Keep all engine access inside adapter; verify headless operation, callbacks, escape side effects, and dependency upgrade effort |
| PTY shutdown and ConPTY differ materially | Hung process/thread or leaked handles in repeated spawn/close tests | Dedicated OS lifecycle fixtures; platform-specific cancellation adapter; no completion claim until handles and children settle |
| Unicode shaping and cell widths disagree | Cursor/selection drift or split wide glyphs | Specify width policy, test fixtures, fallback font behavior; treat terminal coordinates as authoritative |
| Decorative workload harms everyday use | Continuous redraw, elevated minimized CPU/GPU, unreadable text | Effects off in baseline; central animation scheduler; reduce-motion mode; disable expensive effect path until budget passes |
| Provenance failure | Unattributed asset, code pasted from excluded project, unclear transitive license | Quarantine contribution; replace independently; require provenance review before merge and distribution |
| Excessive feature scope | Plugins/remote services requested before basic shell quality | Keep deferred list explicit; require a new milestone and security/resource review |
| Cross-platform support is assumed from compilation | IME, Wayland, DPI, or installers fail manually | Native OS runners plus real desktop smoke testing; publication limited to tested targets |

## Implementation workflow and staffing

One owner maintains cross-crate contracts and integrates changes. Use narrow PRs with acceptance evidence, keep dependencies intentional, and record decisions with rationale. Every commit follows the repository's Lore trailer convention. No application implementation should begin merely because this plan exists: M0 is the first executable work package and later milestone entry depends on its evidence.

Future agent or developer lanes can run concurrently after contracts stabilize:

- Terminal lane owns PTY/engine adapter and lifecycle fixtures.
- Rendering lane owns viewport rendering, text layout, atlas, and performance instrumentation.
- Application lane owns component state, settings, themes, layout, and commands.
- Platform/release lane owns OS adapters, packaging, CI, and smoke test scripts.
- Independent reviewer checks interfaces, security policy, compatibility claims, and benchmark evidence.

Assign explicit file ownership and avoid simultaneous edits to shared domain types. Use mocks and contract tests to permit parallel work. A single owner integrates terminal/render changes first; the architecture should not depend on an agent orchestration framework at runtime.

## Review and handoff

Review order: planner draft → architecture review of feasibility and interfaces → critic review of alternatives, testability, and missing failure modes → owner revision. Deliverable completion means documentation is internally consistent and implementation-ready; it does not mean the MVP exists or its targets have been met.

The public documentation set should cross-link architecture, dependency decisions, rendering, terminal protocol/lifecycle, configuration/themes, platform strategy, security/provenance, tests/performance, development workflow, release packaging, and a requirements-to-gates matrix. The accompanying test specification supplies exact fixtures and release evidence requirements. Update both plan and test specification if an accepted ADR changes a gate.

## Follow-up orchestration (optional)

Available relevant native agent types: default, planner, architect, executor, debugger, test-engineer, security-reviewer, verifier, critic, writer. Use supported inherited models if named role model settings are unavailable. Suggested reasoning: high for architecture/terminal/render/security, medium for UI/docs/release, high for independent verification.

Single-owner path: `$ralph implement M0 from docs/IMPLEMENTATION_PLAN.md and .omx/plans/test-spec-independent-terminal.md`. One executor owns the spike and one independent verifier checks evidence; do not launch until the implementation task is requested.

Coordinated path after M0: `$team implement M1 from docs/IMPLEMENTATION_PLAN.md with explicit platform, terminal and UI ownership`. Start with three implementation lanes and one integrating owner, not a worker for every crate. Treat this as a workflow hint, not a shell command or a launch performed by this documentation task. Runtime worker settings must be resolved from the installed team skill.

Before team shutdown: integrate contracts, run the required platform/terminal tests, collect benchmarks and provenance, and list unresolved limitations. A subsequent single owner independently reruns the milestone acceptance suite from the integrated revision before marking completion. Compilation alone is insufficient.

## Review changes incorporated

Architecture review retained the provisional toolkit choice and required explicit nonblocking reply saturation, resize transaction ordering, accessible terminal text in MVP, and bounded search/snapshot retention. These contracts are now specified in TECHNICAL_SPEC and covered by TESTING. The strongest counterargument is that toolkit integration may cost more than its settings widgets save; M0 compares actual terminal fixtures, not general GUI demos.

Final independent critic verdict: APPROVE for documentation completion. Outstanding feasibility and schedule risks remain explicit M0 work; this review is not validation of a running application.
