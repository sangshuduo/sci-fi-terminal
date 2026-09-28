# Verification strategy and performance specification

**All cases below are planned. None constitutes an executed application test.** The current deliverable can validate documentation links and TOML syntax only. Milestone gates in [the plan](IMPLEMENTATION_PLAN.md) remain the source for numerical acceptance; this document defines how to measure them.

## Traceability and acceptance cases

| Requirement | Implementation area | Required evidence / milestone |
|---|---|---|
| Independent design | Provenance ledger, PR template, notices | Every imported artifact has origin/license; excluded-source declaration; M1 onward |
| Rust-native/GPU | App shell, render module | Native process without browser runtime; one GPU device, rendered terminal and controls; M0 |
| Three platforms | Platform adapter/CI/packages | Real PTY and installed desktop smoke on Windows, macOS, Linux Wayland and X11; M0/M4 |
| Modern terminal | Core/session/input | Golden byte-stream fixtures and application matrix; M2 |
| Themes/customization | Config/UI/render | Three themes, high contrast, user TOML, live font/palette, rollback after invalid settings; M3 |
| Layout/panels | Pane tree and registry | Tabs, splits, focus and persisted ratios; metrics disable stops sampling; M3 |
| Usability/accessibility | Actions, settings, semantic text | Keyboard walkthrough, IME, accessibility matrix; M0/M3/M5 |
| Efficiency | Scheduler, atlas, buffers | Benchmark report against plan budgets; M0 baseline, M4/M6 gates |
| Extensibility | Panel registry | Register test-only panel without modifying parser/session internals; M3 |
| Security/reliability | Policy boundary/lifecycle | Host-effect denial tests, fuzz corpus, resource bounds, failure recovery; M2/M5 |
| Production distribution | Packaging/signing/release | Signature verification, clean-machine install/upgrade/uninstall, SBOM; M6 |

### M0 go/no-go fixture

Build an original disposable vertical spike: one window with settings text input and a GPU terminal viewport, one shell, IME composition, a wide-character sample, a small accessible text representation, and a resizeable splitter. Use the same fixture on each OS. Prove a public supported renderer integration without a second GPU device or GUI fork. Exercise blocked read/write cancellation, controlled child descendants, 100 spawn/close cycles, fractional scaling, 100 resizes, minimize/restore, and injected surface/device loss. Record actual dependency versions and API constraints.

IME gate: compose and commit Chinese/Japanese text, use dead keys and AltGr where available, and observe exactly one committed string with candidate window at the caret. Accessibility gate: native tool inspection discovers labeled controls, terminal text and caret, and a screen reader can reach the terminal. Full production announcement behavior can follow, but a missing viable semantic bridge fails stack selection. Idle scheduling gate: no continuous redraw with effects and cursor blink off; record CPU/resource baseline. If a core gate fails, try egui on the same fixture within the time box, then revise architecture rather than carrying an undocumented blocker.

## Test layers

1. Unit: grid/snapshot conversion, key encoding for terminal modes, action precedence, schema validation, token resolution, layout tree constraints, selection/history anchor changes, and URI/clipboard policies.
2. Property tests: arbitrary byte chunking yields the same final model; valid resize keeps cursor within bounds and wide cells paired; queue/history accounting respects limits; theme parsing never panics; migration is deterministic and idempotent at current version.
3. Headless integration: fake transport emits partial UTF-8, split escapes, partial writes, EOF, stalls, errors and exit races. Assert ordered replies/input, last snapshot integrity, bounded cancellation and no stale-session effects. Add the cyclic-pressure case where a child writes output but does not read input, fills both write reserves, and must fail visibly without model/GUI deadlock. Test resize success/failure/stall with interleaved output, successive epochs and old hit tests. Hold search/snapshot references during history eviction and assert total retained allocations remain bounded. Golden grids use independently authored sequences from protocol specifications, not another application's tests.
4. Native integration: actual shells, tty dimensions, interrupt handling, application cursor/mouse modes, alternate-screen restore, reaping, lost-child handling, and explicit Windows handle/Job Object tests. Run 100 start/stop cycles and verify resources settle; record escaped detached descendants as a known OS policy limitation.
5. GPU/desktop: deterministic render fixtures at 1×, 1.25×, 1.5× and 2× scale, clipping, atlas eviction, Unicode and selection. Use structural layout assertions plus screenshot tolerance; driver/font rasterization prevents universal byte-identical screenshots. Pin fixture fonts with documented licenses before golden image baselines. Validate API errors and recovery on real hardware; a headless software adapter cannot prove driver performance.
6. End-to-end: new tab/split, run shell command, copy/search, change font/theme, invalid config recovery, export/reset, close live session, restart into layout, settings keyboard navigation. Test with NVDA, VoiceOver and Orca on their respective supported systems for production.
7. Fuzzing: parser adapter, escape policy, config/theme decoder and resize/snapshot state machine. Nightly 15-minute smoke per target corpus; before beta at least 24 CPU-hours cumulative per parser/config target with memory limits and no unresolved crashes. Preserve minimized reproductions. Fuzzing does not prove absence of defects.
8. Observability: typed counters for queue depth, dropped/coalesced notifications, parsed bytes, model batches, redraws, atlas/target bytes, session states and callback duration. Assert no sensitive payload in default logs. Hardware GPU timing may be unavailable: report that gap rather than substituting CPU submission time.

## Performance protocol

Reference host classes and budgets are in [the plan](IMPLEMENTATION_PLAN.md). M0 must fill `benchmarks/baseline.md` with exact CPU/GPU/RAM, OS/driver, backend, display, toolchain/lockfile, build flags, power mode, font and process measurement definitions. Freeze those settings for comparisons; use a baseline from the same machine. Keep child-shell CPU/RAM separate and additionally report combined session cost. Track OS footprint and allocator accounting separately; shared GPU memory must not be double-counted silently.

Proposed final numerical GPU cap: ≤128 MiB accounted app-owned buffers/textures at 1080p with one pane/effects off; atlas ≤64 MiB shared across panes. At 4K or four panes, report scaling and enforce ≤256 MiB total app-owned GPU resources. These exclude opaque driver allocations, which must be reported separately when tools expose them. Confirm feasibility in M0; changes require an ADR. History stress uses the configured 32 MiB/session cap plus fixed viewport/queue overhead, not the empty-session idle-memory threshold.

- Startup: 30 warm launches for MVP (50 production), from process entry to first editable viewport; report shell-prompt readiness separately using a no-profile test shell marker. Cold starts: 20 trials under a recorded cache-reset/reboot procedure; report raw samples and empirical p95, acknowledging small-sample uncertainty. Do not call a warm filesystem-cache run cold.
- Input: collect at least 10,000 timed input events with a controlled local echo fixture, measure dispatch→model→GPU submission and p50/p95/p99. Submitted-frame latency is not photon latency; sample high-speed-camera or platform presentation telemetry separately before production.
- Render: scroll the same 120×40 corpus for 60 seconds at 60 Hz; capture CPU frame work and supported GPU timestamps/presentation misses. Repeat with four panes, effects on/off and 4K separately. No effects-on result may replace the baseline.
- Idle: settle 30 seconds, measure 60 seconds with cursor/effects disabled; normalize 100% CPU to one logical core. Repeat focused, unfocused, minimized, metrics on/off, and battery mode. Record wakeups and redraws, not just CPU averages.
- Throughput: deterministic producer sends 10 MiB/s ASCII+SGR for 60 seconds, with sequence markers checked after parse; no bytes may disappear under coalescing. Production: 30 minutes and eight sessions at **10 MiB/s aggregate**, evenly distributed, with one visible pane receiving input. p99 input submission target ≤50 ms during stress; the stricter normal-load production target remains separate.
- Memory: sample baseline, full scrollback, output flood, 100 resize cycles and 100 session cycles. After warm-up and cache settling, growth over the final ten repeated cycles must be ≤5 MiB CPU and ≤5 MiB accounted GPU, without a positive trend across repeated suites. A finite cache plateau is acceptable only within caps.

Collect medians and percentiles, raw trace artifacts, and failure explanations. Performance regressions >10% versus the same-host baseline require investigation even if absolute budgets still pass. Do not make timing assertions on oversubscribed hosted CI; use dedicated/nightly hosts and manual release evidence.

## Pre-mortem and release blocking

| Failure scenario | Early test and prevention | Release consequence |
|---|---|---|
| Custom terminal looks good but IME/accessibility unusable | M0 semantic/candidate-position prototype on all OSes | Change toolkit before UI investment; core accessibility failures block production |
| Output flood or hostile escape causes hang/memory growth | Bounded parser/queue tests, fuzzing, eight-session stress and stalled writer | Block MVP until bounds and cancellation are proven |
| Packaged build differs from developer machine | Signed installed-artifact tests on clean machines, assets/loader/signature checks | Block release; source build success is insufficient |

Every milestone report records revision, platforms actually exercised, command/test outputs, hardware, unmet gates, owner and follow-up. No waived security, corruption, input-loss, or process-hang defects at MVP. Cosmetic defects can be documented; unsupported OS targets must not be advertised as supported. An independent reviewer verifies M4 and M6 evidence against this checklist.
