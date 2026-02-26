# AGENTS.md — Guide for AI Agents and Contributors

This repository hosts DevIt (Rust-based secure orchestration for AI coding) and several subprojects (MCP server, CLI/tools, and a separate IDL 4.2 parser: hdds_gen). This document gives you the mental map, how to run things, where CI lives, and the local conventions to follow when editing code.

If you’re resuming work: first glance at projects/hdds_gen/STATUS.md and this AGENTS.md.

## Repository Map (top level)

- README.md — Product overview for DevIt (daemon, MCP server, security model, quick start)
- crates/ — Rust workspace crates for DevIt
  - mcp-server — HTTP/SSE server exposing DevIt tools to MCP clients (Claude Desktop, etc.)
  - mcp-core, mcp-tools — shared MCP tool plumbing
  - cli, tui — user interfaces/entry points (where present)
  - sandbox, backends, orchestration, tools — security, worker orchestration, and utilities
- devitd/ — Persistent daemon (process manager, screenshots/OCR, orchestration)
- devitd-client/ — Client bits for daemon interop
- devit-landing-preview/ — UI/preview assets
- scripts/ — Helper scripts (platform and integration)
- projects/hdds_gen/ — OMG IDL 4.2 parser + codegen (independent subproject)
  - README.md — Feature set and developer docs for the parser
  - STATUS.md — Quick WIP/DONE snapshot and CI links
  - project_tracking/ — WIPs and DONE_WIP_*.md archives
  - .gitea/workflows/ci.yml — CI for the parser (fmt/clippy/tests + suites)
  - src/ — Parser, types, validators, pretty, codegen
  - examples/ — Canonical IDL, invalid suites, macros, interfaces

## Build and Run

Workspace (DevIt)
- Build all: `cargo build --workspace`
- Daemon: `./target/release/devitd --socket /tmp/devitd.sock --secret "$DEVIT_SECRET"`
- MCP server (HTTP+SSE):
  - `./target/release/mcp-server --transport http --host 0.0.0.0 --port 3001 --working-dir . --enable-sse`
  - Manifest served at `/.well-known/mcp.json` with transport URLs (`/message`, `/sse`)
  - SSE must emit an initial `event: ready` + periodic heartbeats
  - If tunneling with ngrok, append `?ngrok-skip-browser-warning=1` to all URLs

Subproject (IDL Parser: projects/hdds_gen)
- Quick dev loop: `make fmt && make clippy && cargo test`
- Canonical fmt-check: CI enforces no drift in `examples/canonical/*.idl`
- Feature flags: `--features interfaces` to run interface tests
- CLI: `idl-gen` supports `parse`, `check`, `fmt`, and `gen {cpp|rust}`

## CI (Gitea Actions)

- Location: projects/hdds_gen/.gitea/workflows/ci.yml
- Steps:
  - rustfmt/clippy/tests
  - Canonical IDL strict fmt-check
  - Invalid suites (IDL invalid and interfaces_invalid) — expect failures
  - Preprocessor macros check — runs `idl-gen check` on examples/macros/*.idl
  - Interfaces feature tests: `cargo test --features interfaces`
- Optional self-hosted job (label `rti`) for RTI comparisons can be added if tools are installed.

## Conventions

- Rust style: `cargo fmt`, `cargo clippy -D warnings` before pushing
- Tests: add focused unit tests near changed code; prefer small repro IDLs for parser
- Docs: update README/STATUS/audit docs when surfacing new behavior or coverage
- Commits: concise subject; body lists scope and side-effects (tests/CI/docs)

## MCP Server Notes (crates/mcp-server)

- Transport: `type: "http"` with `url: /message` (JSON-RPC) and `sseUrl: /sse` (Server-Sent Events)
- SSE requirements:
  - Write `event: ready` immediately on connect
  - Send regular `: keep-alive` comments and flush writes (no compression)
  - Upstream proxy to the binary must be HTTP/1.1 (chunked) with flush pass-through
- Manifest: serve at `/.well-known/mcp.json`; if using ngrok, include `?ngrok-skip-browser-warning=1` in the manifest URLs

## IDL Parser (projects/hdds_gen) — Key Files

- src/parser.rs — Pratt parser (const expr), structs/unions/typedefs/enums, bitset/bitmask
- src/validate.rs — Structural and annotation rules; now includes interface validation (feature)
- src/pretty.rs — IDL pretty printer (fmt)
- src/codegen/{cpp.rs,rust_backend.rs} — Code generators (DDS-friendly C++/idiomatic Rust)
- src/bin/idl-gen.rs — CLI, with a pragmatic preprocessor (includes, `#define`, conditionals, function-like macros, basic `#`/`##`)
- examples/* — Canonical, invalid, macros, interfaces
- project_tracking/* — WIPs and DONE archives; see STATUS.md for the snapshot

## Preprocessor Scope (IDL parser CLI)

Provided for IDL authoring convenience; not a full C preprocessor.
- Includes (quoted/angled) with -I; cycle guard
- Object-like defines; function-like macros with parameter substitution
- Basic `#` stringize (quotes argument text) and `##` token-pasting (concatenate tokens)
- One-pass expansion of object-like macros after function-like expansion

Future refinement (tracked in WIP_PREPROCESSOR_ADVANCED.md)
- Escaping in stringize
- Re-tokenization for token-paste
- No expansion inside string/char literals; recursion guards

## How to Pick Up Work

1) Read projects/hdds_gen/STATUS.md for DONE/ACTIVE WIPs
2) For parser tasks: run `make fmt && make clippy && cargo test` locally
3) Add minimal repros to examples/ or unit tests close to the code
4) Update docs (README, audit) if behavior or coverage changes
5) Keep patches focused; avoid unrelated refactors

## Troubleshooting

- SSE newlines: do not include newlines in SSE field values; use separate `data:` lines; no compression on `/sse`
- ngrok banner: add `?ngrok-skip-browser-warning=1` to all MCP URLs
- CI failures on fmt: run `idl-gen fmt` locally and sync canonicals
- Windows PowerShell curl quirks: prefer `curl.exe` with explicit quoting; see docs/windows_daemon_setup.md

