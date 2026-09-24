# Security, independent design and provenance

## Clean-room boundary

This project uses eDEX-UI only as the user's high-level reference for a visually rich terminal experience. Do not retrieve, inspect, copy, translate, decompile, port, or adapt its source, implementation documentation, shaders, assets, themes, tests, or project-specific components. Do not ask generators or agents to reconstruct its code or pixel-match its screens. Do not use its branding or imply endorsement, succession, or affiliation.

Requirements, UI composition, architecture, test fixtures, artwork, and documentation are authored independently. Standard terminal protocols and independently licensed libraries are legitimate foundations; standards compatibility is not a requirement to reproduce another project's implementation. The term clean-room here describes this documented exclusion process, not a legal certification or a claim about every contributor's past exposure.

Contributor process: record the requirement/standard or independent source behind each substantial implementation; include a PR provenance statement and source/license links for every new asset or dependency. Contributors with prior eDEX implementation exposure disclose it to maintainers and avoid translating remembered details; assign the affected component to an unexposed author if necessary. If suspect material arrives, quarantine the change, stop its integration, document the issue, and independently reauthor from approved requirements. Keep a provenance ledger with artifact path, author, origin URL if external, version/hash, SPDX identifier, modifications, and required attribution. AI-assisted work follows the same provenance and review requirements.

Propose MIT OR Apache-2.0 for original application code, subject to the owner's license decision before public distribution. Do not publish a LICENSE claiming a decision that has not been made. Include full license texts and attribution for the exact dependency graph and bundled assets. Prefer original vector icons; use system monospace fonts initially, and bundle a font only after checking its specific license (including reserved font names and redistribution conditions). Produce a release SBOM and third-party notice bundle. A permissive top-level crate license alone does not establish compatibility of all transitive/native dependencies.

## Trust boundaries and policies

| Input / boundary | Threat | Required control |
|---|---|---|
| Shell/remote PTY output | Malformed escapes, memory exhaustion, deceptive titles/links | Fuzz parsing; payload/history caps; sanitize labels; no implicit host effects |
| Clipboard and paste | Exfiltration or accidental command execution | Deny OSC52 by default; explicit paste preview policy; no clipboard logging |
| Config/theme files | Startup command injection or unsafe resource loading | Typed bounded data; no auto project config; argv spawning; themes cannot execute |
| Local shell child | Access to user files/network | Runs at user's privileges; disclose normal terminal trust, no false sandbox claim |
| GPU/font parsers | Driver crashes or malformed fonts | Trusted local/system fonts, resource bounds, device recovery, pinned reviewed dependencies |
| Release/update channel | Malicious binary replacement | Signed artifacts, checksums, protected build credentials, verified provenance |
| Future extensions | Terminal spying/input injection | Explicit capabilities, isolation/quotas, versioned protocol before deployment |
| Process/network monitor | Leaking other processes' arguments or secrets; surveillance of connections | Collect name/PID/CPU/memory only (never command lines or environment); sockets read locally, capped at 200; nothing persisted or sent |
| Public-IP lookup (opt-in) | Disclosure of the user's IP to a third party; hostile or spoofed response | Off by default; https-only endpoint, no redirects, 5 s timeout, 64-byte body cap; response must parse as a single public IP; never rendered as markup or executed |
| GeoIP database | Network exfiltration of peer addresses; malformed database | Offline `.mmdb` only, opt-in path, ≤ 512 MiB, public addresses only, bounded lookup cache; no downloads or online lookups |
| Directory viewer | Accidental execution, path confusion | Read-only listing (≤ 500 entries), symlinks shown not followed for listing metadata; "Insert `cd`" types a quoted command without Enter |
| On-screen keyboard layouts | Hidden command strings | Data-only TOML; keys map to single characters, named keys or modifiers only |
| Sound | Distraction, device hangs | Off by default, synthesised in code, rate-limited, non-blocking; failure to open a device is silent |

No telemetry or network access by the app by default. The single exception is the opt-in public-IP lookup (ADR-007). It is off by default. When enabled, it sends one HTTPS GET, at most every 30 min, to a user-configurable endpoint, and that endpoint necessarily learns the user's IP address. Settings say so. child commands retain normal network access. Metrics are local only: CPU, memory, swap, top processes (without arguments), interface counters and local socket tables. Do not persist terminal output or history to disk. Diagnostics include versions, backend, dimensions, timing and typed error codes, never terminal bytes, clipboard, command arguments, environment values, or unrestricted file paths. Debug traces containing sensitive payloads must not exist in release defaults. Support bundles are opt-in, previewable and redacted; crash dumps may contain secrets and require explicit export consent.

Custom keybindings bind known actions, not arbitrary command strings in MVP. User-defined shell profiles intentionally execute programs; show that fact clearly in settings and do not import profiles from themes or remote files. URI opening requires explicit activation, visible target and an allowlisted scheme. Reject terminal file-write and external command escape extensions. See [terminal policy](TERMINAL.md).

## Release hardening and response

Prefer safe Rust in project code. Isolate any required unsafe/FFI in `platform`, document safety invariants and ownership, review it separately, and exercise handle cleanup under failure. Maintain limits even if an upstream parser claims trusted input. Test partial writes, reentrant callbacks, very long control strings, output floods, stalled children and device loss.

Protect release signing credentials in restricted CI environments; untrusted pull requests do not receive secrets. Lock and review third-party CI actions, minimize workflow token permissions, generate checksums/SBOM/provenance after build, then sign immutable artifacts. No auto-updater in MVP; production updater requires a separate threat model, authenticated metadata, rollback protection and explicit UX.

Before public beta, publish a private vulnerability reporting route and named triage owner. Targets: acknowledge within 3 working days, initial severity assessment within 7; these become commitments only when staffed. Security-blocking defects prevent release; dependency advisory exceptions require an owner, impact analysis, expiry and mitigation. There is no current security review of executable code because none exists.
