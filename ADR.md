<!-- markdownlint-disable MD013 MD036 -->

# Bunch of Tools: Architecture Decision Record and Delivery Ledger

| Field          | Value                                                            |
| -------------- | ---------------------------------------------------------------- |
| Document       | ADR-000                                                          |
| Status         | Living document; foundational decisions accepted                 |
| Product name   | `Bunch of Tools` (working name)                                  |
| Repository     | `F:\_Engine\bunchoftools`                                        |
| Created        | 2026-07-16                                                       |
| Last updated   | 2026-07-16                                                       |
| Primary target | Native Windows desktop                                           |
| Later targets  | Self-hosted web application; constrained browser-local execution |

## Navigation

- [Executive summary](#0-executive-summary)
- [Context and product definition](#2-context-and-problem-statement)
- [Architectural principles](#4-architectural-principles-and-invariants)
- [Decision index and records](#6-decision-index)
- [System architecture](#8-system-architecture)
- [Repository and crate plan](#9-repository-and-crate-plan)
- [Domain and capability model](#10-domain-model)
- [GPUI desktop architecture](#13-gpui-desktop-architecture)
- [Web and server architecture](#14-web-and-server-architecture)
- [Security and file safety](#15-security-and-privacy-model)
- [Validation strategy](#18-validation-strategy)
- [Milestone plan](#19-milestone-plan)
- [Risks and open questions](#21-risks-and-mitigations)
- [Chronological decision log](#24-chronological-decision-log)

## Current Checkpoint

| Item                            | Current state                                                    |
| ------------------------------- | ---------------------------------------------------------------- |
| Current checkpoint              | M2: Reopened after end-to-end product review                     |
| Next implementation milestone   | M2: Complete the image workbench acceptance gates                |
| Accepted foundation             | Rust core, GPUI desktop, WASM facade, TypeScript web renderer    |
| Immediate decision gate         | M2 image workflow and desktop UX remediation                     |
| Intentionally unresolved        | Other engines, bundling, remote file model, browser-local subset |
| Current implementation blockers | Product acceptance gaps recorded in the reopened M2 section      |

## 0. Executive Summary

Bunch of Tools is a local-first project asset workbench for the recurring file operations that interrupt development work: image conversion and compression, GIF and video preparation, frame and cover insertion, lightweight timeline composition, PDF manipulation, Markdown and document conversion, metadata handling, batch work, and repeatable project-specific recipes.

The application is not intended to replace Photoshop, DaVinci Resolve, a full document editor, or other deep creative suites. Its purpose is to complete common asset-preparation jobs quickly, privately, reproducibly, and without requiring a collection of upload-based web services.

The accepted architectural direction is:

```text
                         Shared Rust brain
              domain / recipes / validation / planning
                       /                       \
                      /                         \
         Native executor                        WASM facade
      filesystem / processes                  browser-safe API
             |                                      |
      GPUI desktop + CLI                    TypeScript renderer
             |
   vetted local tool adapters
```

For the hosted web target, the browser may use the WASM build for immediate recipe editing and validation, but the Rust server remains authoritative. The server revalidates the recipe, constructs its own execution plan, and runs the native adapters. A plan produced by an untrusted client is never executed.

The central contract is a versioned, typed recipe. Raw shell commands are not the product API. GPUI and TypeScript are renderers over the same Rust concepts, not separate implementations of the product rules.

## 1. Purpose and Governance

### 1.1 Purpose

This file is the canonical destination for:

- product boundaries and non-goals;
- accepted, proposed, deferred, superseded, and rejected decisions;
- system and crate architecture;
- native, server, and browser execution models;
- security and privacy requirements;
- file-safety and persistence contracts;
- user-interface structure and interaction principles;
- milestones and acceptance gates;
- validation commands and release criteria;
- known risks, unresolved questions, and future investigations;
- a chronological record of material architectural changes.

Other documents may explain a feature or provide user instructions, but they must not silently redefine an architectural decision recorded here.

### 1.2 Status vocabulary

Every material decision uses one of these statuses:

- `Accepted`: binding for implementation until explicitly superseded.
- `Proposed`: preferred direction that still needs evidence or approval.
- `Deferred`: intentionally postponed; it must not block current milestones.
- `Rejected`: considered and deliberately not selected.
- `Superseded`: replaced by a newer decision that names the old decision.
- `Experimental`: allowed only in a bounded spike and not production truth.

Milestones use these statuses:

- `Not started`
- `In progress`
- `Blocked`
- `Complete`

`Complete` means that all listed exit gates have passed. A partial demo is not a completed milestone.

### 1.3 Change procedure

When a material decision changes:

1. Add a new numbered decision or amend an existing `Proposed` decision.
2. Do not erase the old rationale.
3. Mark replaced accepted decisions as `Superseded` and link the replacement.
4. Update the architecture, milestone, risk, and validation sections affected.
5. Add a dated entry to the decision log in Section 24.
6. Record evidence, including commands, measurements, compatibility results, or user acceptance criteria that justify the change.

Small implementation details do not require a decision record unless they alter a public contract, security boundary, persistence format, distribution model, or cross-target behavior.

### 1.4 Current project state

As of 2026-07-16:

- M0 and M1 are complete; M2 is reopened and in progress after product review;
- the repository contains a pinned Rust workspace, shared domain, planner, and protocol crates, a native execution crate, a CLI, and a GPUI desktop shell;
- Rust, GPUI, WASM, and TypeScript boundaries are accepted;
- ImageMagick 7 is accepted only for the scoped native `image.resize` adapter;
- qualified static-image probing, source proxy preview, resize, crop, conversion, metadata policy, staged publication, and artifact review form a verified engine foundation shared by GPUI and CLI;
- all other processing-engine selections and engine bundling remain unresolved;
- the deterministic `foundation.simulate` operation remains an explicit contract harness and does not claim to process media;
- the initial native quality gates, portable WASM compilation, image fixtures, and real CLI execution pass, but the GPUI workflow does not yet satisfy M2;
- M2 remains the active milestone. M3 does not begin until the image workbench passes the reopened workflow, batch, export, layout, and visual acceptance gates.

## 2. Context and Problem Statement

Development work regularly requires small but varied transformations:

- compressing images before committing or publishing them;
- converting among PNG, JPEG, WebP, AVIF, GIF, and project-specific formats;
- resizing, cropping, padding, framing, or composing images;
- inspecting or removing metadata;
- extracting, inserting, replacing, or reordering video frames;
- adding a still cover or title card to a recording;
- trimming and joining clips without opening a full video editor;
- preparing short GIFs or release demonstrations;
- merging, splitting, rotating, compressing, or rearranging PDFs;
- converting Markdown, HTML, DOCX, EPUB, and related document formats;
- applying the same transformation to many files;
- repeating a known asset-preparation sequence for every release.

Existing options tend to impose one or more costs:

- uploading private or unpublished files to third-party services;
- uncertain retention, analytics, or security practices;
- inconsistent quality controls and output naming;
- repeated manual work with no reproducible recipe;
- application startup and project setup disproportionate to a small task;
- different interfaces for each file category;
- no reliable way to replay the exact transformation later;
- loss of metadata, color-profile, timing, or codec intent without warning.

The problem is therefore not simply a missing converter. It is the absence of a trusted, low-friction preparation layer between raw project assets and assets ready to commit, document, publish, or share.

## 3. Product Definition

### 3.1 Product statement

Bunch of Tools is a compact native workbench that turns selected project files into reviewed output artifacts through explicit, local, repeatable operations.

### 3.2 Primary user

The primary user is a developer or technical creator who:

- works across code, documentation, screenshots, recordings, PDFs, and media;
- values local processing and transparent output;
- wants quick controls rather than a professional editing suite;
- sometimes needs batch or scripted execution;
- expects the GUI and CLI to agree;
- may later want the same recipes available through a private hosted service.

### 3.3 Goals

1. Keep ordinary desktop processing local by default.
2. Make common transformations discoverable without hiding their consequences.
3. Produce deterministic, reviewable execution plans from typed recipes.
4. Reuse mature processing engines instead of recreating codecs and parsers.
5. Support cancellation, progress, error recovery, and safe outputs as core behavior rather than per-tool afterthoughts.
6. Provide a fast GPUI desktop renderer with no embedded webview.
7. Share the Rust domain and planning logic with a later WASM export.
8. Support automation through the same contracts used by the desktop app.
9. Make useful workflows portable between projects and machines.
10. Remain extensible without becoming an arbitrary shell-command launcher.

### 3.4 Non-goals

The initial product will not:

- replace full nonlinear video editors;
- replace professional photo-development or layer-based image editors;
- implement media codecs, PDF parsers, or office formats from scratch;
- promise every format supported by every installed engine;
- execute arbitrary user-provided shell fragments as recipes;
- upload desktop files to a hosted service by default;
- promise feature parity between native and browser-local execution;
- include collaboration, accounts, cloud storage, or marketplace features;
- silently overwrite input files;
- silently strip metadata, color profiles, audio, subtitles, or document data;
- treat a file extension alone as trustworthy proof of content type.

### 3.5 Product success criteria

The product is succeeding when:

- a common preparation task can be completed without opening a specialist app;
- the user can see input, intended operations, output destination, and expected consequences before execution;
- the same recipe produces equivalent plans in GPUI, CLI, server, and WASM planning contexts where the capability is available;
- cancellation does not leave a misleading success state or corrupt output;
- output is reviewable before replacing or publishing anything;
- processing remains useful without an account or internet connection;
- failures identify the failing operation and engine instead of exposing only a raw command exit code.

## 4. Architectural Principles and Invariants

These rules apply unless superseded by a later accepted decision.

1. **One domain model.** Operations, recipes, validation, and planning live in shared Rust crates.
2. **Renderers are adapters.** GPUI and TypeScript render application state; neither owns processing truth.
3. **Execution is target-specific.** Native processes, server processes, and browser APIs have different capabilities behind a common conceptual model.
4. **Recipes are declarative.** A recipe describes intent, not a shell script.
5. **Plans are derived.** Concrete engine invocations are generated, ephemeral, and never trusted when received from a client.
6. **Inputs are untrusted.** Local provenance does not make a malformed media file safe to parse.
7. **Outputs are non-destructive by default.** Input replacement requires an explicit operation and a recoverable strategy.
8. **Blocking work stays outside UI execution.** Processing, probing, hashing, thumbnail creation, and large file I/O do not run on the GPUI render path or browser main thread.
9. **Cancellation is part of every long operation.** It is not optional adapter behavior.
10. **Stale results cannot win.** Preview generations and job identities prevent
    superseded work from replacing current state.
11. **Capabilities are discovered.** The UI only offers operations supported by
    the active executor and qualified engine versions.
12. **No hidden network dependency.** Core desktop workflows remain available
    offline after required engines are installed or bundled.
13. **Cross-target parity is explicit.** Unsupported target capabilities fail
    clearly; they do not degrade silently.
14. **Persistence is versioned and crash-aware.** Recipes and settings are never
    unversioned implementation dumps.
15. **Security policy is code and configuration.** It is tested and packaged,
    not left to whatever defaults an external engine happens to ship.

## 5. Local Precedents and Lessons

The architecture is grounded in three existing GPUI projects in the same local development root. They are evidence and design precedents, not dependencies.

### 5.1 Woodpecker

Relevant lessons:

- GPUI presentation is kept outside the timing and native-input engine path.
- Native work reports structured snapshots and events to the UI.
- Dedicated channels and threads isolate operating-system integrations.
- Persisted state is sanitized and written atomically by a coalescing worker.
- GPUI and `gpui-component` versions are pinned for reproducibility.

Applied here:

- processing adapters do not mutate GPUI state directly;
- a job runner publishes typed events;
- settings writes are coalesced and crash-safe;
- external-engine interaction has a narrow boundary.

### 5.2 DevHub GPUI

Relevant lessons:

- the reusable core and GPUI application are separate crates;
- cancellable background work uses explicit cancellation tokens;
- generation numbers prevent old results from replacing new state;
- configuration and caches have schema versions and crash-safe migration;
- platform-specific behavior stays behind a dedicated platform module.

Applied here:

- recipe planning and execution remain outside the GPUI crate;
- preview and probe requests have generation identities;
- settings and recipe migrations reject unknown future schemas;
- Windows process and filesystem behavior stays in native platform modules.

### 5.3 Chessengine

Relevant lessons:

- one authoritative Rust library serves multiple frontends;
- native-only and web-compatible functionality is feature-gated;
- the GPUI application can use a native process boundary;
- a dedicated `wasm-bindgen` adapter exposes structured operations;
- the TypeScript application runs heavy WASM work in a dedicated worker;
- worker commands and responses are typed and correlated by request identity;
- hard cancellation may require terminating and recreating a worker;
- native and WASM behavior is checked against shared correctness oracles.

Applied here:

- `bunch-wasm` is an adapter over shared crates, not a second implementation;
- TypeScript never reimplements recipe validation or planning rules;
- browser work does not block the rendering thread;
- parity tests compare serialized recipes, plans, validation, and estimates;
- native-only adapters are feature- and crate-isolated.

### 5.4 Lesson not to copy

The existing applications also demonstrate a scaling limit: their main GPUI state and rendering code became concentrated in large source files. Bunch of Tools has a broader surface, so it will split application state, tool views, preview surfaces, inspectors, and queue rendering before they become a single monolithic view.

## 6. Decision Index

| ID    | Decision                                            | Status   | Date       |
| ----- | --------------------------------------------------- | -------- | ---------- |
| D-001 | Local-first project asset workbench                 | Accepted | 2026-07-16 |
| D-002 | Rust owns the domain brain                          | Accepted | 2026-07-16 |
| D-003 | GPUI is the native desktop renderer                 | Accepted | 2026-07-16 |
| D-004 | WASM plus TypeScript is the web renderer            | Accepted | 2026-07-16 |
| D-005 | Execution backends are target-specific              | Accepted | 2026-07-16 |
| D-006 | Recipes and plans replace raw shell workflows       | Accepted | 2026-07-16 |
| D-007 | Mature engines are used through narrow adapters     | Accepted | 2026-07-16 |
| D-008 | Hosted servers revalidate client recipes            | Accepted | 2026-07-16 |
| D-009 | Structured progress, cancellation, and generations  | Accepted | 2026-07-16 |
| D-010 | Versioned JSON recipes and crash-safe settings      | Accepted | 2026-07-16 |
| D-011 | Windows-first, cross-platform-shaped implementation | Accepted | 2026-07-16 |
| D-012 | GPUI version pinning and UI dependency isolation    | Accepted | 2026-07-16 |
| D-013 | Initial processing-engine set                       | Proposed | 2026-07-16 |
| D-014 | Engine discovery first, bundled engines later       | Proposed | 2026-07-16 |
| D-015 | SQLite job history                                  | Deferred | 2026-07-16 |
| D-016 | Browser-local heavy processing                      | Deferred | 2026-07-16 |
| D-017 | Remote server file-access model                     | Proposed | 2026-07-16 |
| D-018 | ImageMagick 7 for scoped native image resize        | Accepted | 2026-07-16 |
| D-019 | Frictionless task-first image surface               | Accepted | 2026-07-16 |
| D-020 | Darktable boundary for RAW development              | Accepted | 2026-07-16 |

## 7. Detailed Decision Records

### D-001: Local-first project asset workbench

**Status:** Accepted
**Date:** 2026-07-16

**Decision**

The primary product is a local desktop workbench for project asset preparation. The default desktop workflow does not upload files or require a hosted account.

**Rationale**

Privacy, context switching, and repeated manual operations are the motivating problems. A cloud-first converter would preserve the primary source of distrust and would not improve offline or repository-oriented workflows.

**Consequences**

- local engine availability and packaging become product responsibilities;
- malformed inputs must be treated as hostile local content;
- output paths, temporary files, disk use, and process lifetime require careful native handling;
- hosted functionality remains optional and separately secured.

### D-002: Rust owns the domain brain

**Status:** Accepted
**Date:** 2026-07-16

**Decision**

Rust owns operation definitions, recipe schemas, capability matching, validation, planning, output policy, job state transitions, and shared error semantics.

**Rationale**

The native application is already Rust-based, Rust can compile to WASM, and a shared core prevents the GPUI, CLI, server, and web renderers from drifting into different products.

**Boundary**

The portable Rust core must not depend on GPUI, Tokio, native filesystem APIs, process creation, Windows APIs, or browser APIs.

### D-003: GPUI is the native desktop renderer

**Status:** Accepted
**Date:** 2026-07-16

**Decision**

The native desktop application uses Rust and GPUI. It does not embed a webview and does not use Tauri or Electron.

**Rationale**

GPUI matches the desired native stack and existing local experience. It offers GPU-accelerated custom rendering suitable for previews and timeline surfaces while preserving a Rust-native application boundary.

**Consequences**

- the project accepts GPUI API churn and limited framework documentation;
- GPUI must be pinned to an exact version;
- custom media surfaces will require more native UI engineering than a web implementation;
- GPUI types must remain confined to the desktop crate.

### D-004: WASM plus TypeScript is the web renderer

**Status:** Accepted
**Date:** 2026-07-16

**Decision**

A later web application uses a TypeScript renderer and a dedicated WASM facade compiled from the shared Rust core. Heavy WASM operations run in workers.

**Rationale**

This preserves the shared domain rules while allowing an interface designed for browser layout, accessibility, file selection, and hosting. GPUI itself is not expected to render in the browser.

**Consequences**

- the GPUI and TypeScript view code are separate implementations;
- protocol parity and generated or verified types are required;
- browser capability differences must be visible in the interface;
- WASM is not assumed to provide native process execution.

### D-005: Execution backends are target-specific

**Status:** Accepted
**Date:** 2026-07-16

**Decision**

The system defines a common execution concept but permits distinct native, server, and browser-local implementations.

```text
Recipe -> validate -> plan -> executor -> job events -> artifacts
```

Native and server executors may spawn vetted tools. A browser-local executor may use Web APIs and WASM libraries only. Unsupported operations are explicit.

**Rationale**

Pretending these environments have identical filesystem, process, codec, threading, and memory capabilities would produce weak abstractions and silent behavior differences.

### D-006: Recipes and plans replace raw shell workflows

**Status:** Accepted
**Date:** 2026-07-16

**Decision**

The persisted and exchanged contract is a typed recipe. A recipe contains operation identifiers and parameters. The planner resolves it to a concrete, executor-specific plan. Users do not save arbitrary command strings as recipes.

**Rationale**

Typed recipes enable validation, previews, migrations, capability checks, security review, deterministic naming, CLI parity, and cross-target planning.

**Consequences**

- engine-specific advanced controls must be introduced deliberately;
- a future expert mode may expose reviewed adapter parameters but not a shell;
- plans include engine versions and resolved arguments for diagnostics but are not a stable portable format.

### D-007: Mature engines are used through narrow adapters

**Status:** Accepted
**Date:** 2026-07-16

**Decision**

The project orchestrates proven media and document engines rather than reimplementing codecs, PDF structures, RAW pipelines, or office conversions. Every engine is wrapped by an adapter that exposes only supported operations.

**Adapter responsibilities**

- discover and report the engine version;
- declare capabilities and relevant limits;
- probe inputs using structured output where available;
- translate typed operations to argument arrays without a command shell;
- parse progress and diagnostics;
- support cancellation and process-tree cleanup;
- verify expected output before publishing it;
- identify licensing and distribution obligations.

### D-008: Hosted servers revalidate client recipes

**Status:** Accepted
**Date:** 2026-07-16

**Decision**

The hosted TypeScript application may use WASM to validate and preview a recipe, but it sends a recipe request rather than an executable plan. The Rust server validates paths and parameters and constructs a new authoritative plan.

**Rationale**

Browser state and network requests are untrusted. Executing a client-generated command plan would turn the server into a remote command-execution boundary.

### D-009: Structured progress, cancellation, and generations

**Status:** Accepted
**Date:** 2026-07-16

**Decision**

All long operations use job identifiers, typed state transitions, structured events, and cancellation. Preview and probe work additionally uses monotonically increasing generation identifiers so stale results are ignored.

**Required states**

```text
Queued -> Preparing -> Running -> Finalizing -> Succeeded
                               \-> Failed
                               \-> Cancelling -> Cancelled
```

**Required event properties**

- job identifier;
- monotonically increasing event sequence;
- operation or step identifier where applicable;
- phase and normalized progress when measurable;
- human-facing summary plus structured diagnostic details;
- timestamps based on a monotonic clock for durations;
- explicit terminal state exactly once.

### D-010: Versioned JSON recipes and crash-safe settings

**Status:** Accepted
**Date:** 2026-07-16

**Decision**

- portable recipes use a human-reviewable, versioned JSON representation;
- application settings use a versioned structured format;
- writes use a temporary file and atomic replacement where supported;
- unknown future schema versions are rejected without rewriting them;
- frequent settings updates are coalesced by a persistence worker;
- paths and machine-local bindings are separated from portable operations where practical.

**Rationale**

Recipes should be commit-friendly and debuggable. Settings need stronger recovery behavior than serializing the live GPUI state tree.

### D-011: Windows-first, cross-platform-shaped implementation

**Status:** Accepted
**Date:** 2026-07-16

**Decision**

The first supported desktop platform is Windows 10/11 x64. Portable crates avoid Windows dependencies. Native platform behavior is isolated so macOS and Linux support remain possible without weakening the first release.

**Consequences**

- Windows process groups or Job Objects, file replacement, long paths, Unicode paths, WebView-independent GPUI requirements, and code signing are first-class;
- cross-platform CI is added only when its target is supportable rather than as a misleading compile-only badge;
- recipes cannot assume Windows path separators or drive letters.

### D-012: GPUI version pinning and UI dependency isolation

**Status:** Accepted
**Date:** 2026-07-16

**Decision**

The initial desktop should align with the locally proven versions:

```toml
gpui = "=0.2.2"
gpui-component = "=0.5.1"
gpui-component-assets = "=0.5.1"
```

The exact versions may change only through an explicit dependency decision and a complete desktop validation pass. GPUI dependencies appear only in the desktop application and UI support crates.

**Rationale**

Woodpecker, DevHub GPUI, and chessengine already use GPUI 0.2.2. Pinning avoids unreviewed framework churn while the product contracts are being established.

### D-013: Initial processing-engine set

**Status:** Proposed
**Date:** 2026-07-16

**Proposal**

Qualify these candidates rather than treating them as already selected:

| Capability                   | Primary candidate  | Alternatives or additions           |
| ---------------------------- | ------------------ | ----------------------------------- |
| Video, audio, GIF, frames    | FFmpeg and ffprobe | Platform media APIs for preview     |
| General image operations     | ImageMagick        | libvips, focused Rust libraries     |
| Metadata                     | ExifTool           | Format-specific libraries           |
| Structural PDF work          | qpdf               | PDFium or MuPDF for rendering       |
| Markdown/document conversion | Pandoc             | LibreOffice headless, Typst         |
| RAW development              | darktable CLI      | LibRaw-based adapter                |
| OCR                          | Not selected       | OCRmyPDF, Tesseract pipeline        |

**Qualification requirements**

- required operations and format coverage;
- structured probe and progress interfaces;
- output correctness on fixed fixtures;
- cancellation behavior;
- Windows distribution and update path;
- license obligations for the exact build configuration;
- security policy and sandbox compatibility;
- startup time, memory use, binary size, and quality controls;
- Unicode and long-path handling;
- deterministic or explainably variable output.

No candidate becomes accepted merely because it is widely installed.

### D-014: Engine discovery first, bundled engines later

**Status:** Proposed
**Date:** 2026-07-16

**Proposal**

Early development discovers explicitly configured or PATH-installed engines and reports exact versions. Distribution later bundles pinned, audited sidecars when licensing, size, provenance, updates, and security policy are understood.

**Reason to defer bundling**

Bundling too early turns a product spike into a codec-distribution and licensing project. Depending permanently on arbitrary PATH tools would make behavior unreproducible. The staged approach gathers capability evidence first, then closes the distribution contract before release.

### D-015: SQLite job history

**Status:** Deferred
**Date:** 2026-07-16

**Decision**

Do not introduce SQLite during the contract and first vertical-slice milestones. The running queue is in memory; portable recipes and settings are files. Add a database only when durable history, indexing, recovery, or cache accounting has demonstrated requirements that files cannot meet cleanly.

### D-016: Browser-local heavy processing

**Status:** Deferred
**Date:** 2026-07-16

**Decision**

The first web target is a self-hosted TypeScript application backed by a native Rust server. Browser-local processing is a later executor with an explicit capability subset. It is not a release-parity commitment.

### D-017: Remote server file-access model

**Status:** Proposed
**Date:** 2026-07-16

**Options under consideration**

1. Uploaded inputs copied into a server-managed job directory.
2. Files already present beneath administrator-configured server roots.
3. Both modes, kept distinct in the protocol and UI.

Arbitrary browser-provided absolute paths are rejected in every option. Remote access beyond loopback requires authentication, authorization, TLS at the deployment boundary, quotas, and explicit retention rules.

### D-018: ImageMagick 7 for scoped native image resize

**Status:** Accepted
**Date:** 2026-07-16
**Related:** D-007, D-013, D-014

**Context**

M2 required one real image workflow with format probing, proxy preview, correctness checks, cancellation, safe output publication, metadata behavior, and equivalent GPUI and CLI semantics. Selecting every future engine under D-013 would be premature, so this decision accepts only the qualified slice.

**Decision**

Use ImageMagick 7 through the `imagemagick.image-v2` native adapter for the static `image.resize` operation. The accepted static input set is AVIF, BMP, HEIC/HEIF, ICO, J2K/JP2, JPEG, JXL, PNG, PSD, QOI, TGA, TIFF, and WebP. The accepted output set is AVIF, BMP, ICO, JP2, JPEG, JXL, PNG, QOI, TGA, TIFF, and WebP. GIF, HEIC/HEIF, and PSD remain read-only; animated or multi-image input is rejected by this operation. The adapter:

- discovers `magick` on `PATH` or uses `BUNCH_IMAGEMAGICK_PATH`;
- requires major version 7 and records the exact engine version in capabilities and artifact manifests;
- invokes the executable directly with typed arguments and never through a shell;
- loads an application-owned deny-first policy, not the installed default;
- generates a per-command policy that denies all paths except declared input, staging output, and the controlled pixel-cache directory;
- denies external delegates, filters, indirect `@` reads, and unqualified coders and modules;
- limits memory to 256 MiB, map to 512 MiB, disk cache to 1 GiB, area to 40 MP, width and height to 8192, image lists to 32, threads to four, and execution to 120 seconds;
- auto-orients, applies fit, fill, stretch, crop, rotation, and flip geometry, makes upscaling explicit, and applies format-appropriate output quality;
- strips profiles and comments by default or preserves them when the recipe explicitly requests preservation;
- writes to same-directory staging, probes the staged result, verifies format, dimensions, and metadata policy, then publishes according to collision policy;
- kills the child before cleaning staging when cancellation is requested.

**Qualification evidence**

The 2026-07-16 Windows qualification used ImageMagick `7.1.2-27` Q16-HDRI x64. PNG, JPEG, WebP, GIF, LCMS, and the required conversion operations were present. On this machine:

- the installed distribution occupied 64.1 MiB; `magick.exe` itself is a small launcher and distribution size is the relevant bundling measurement;
- five version probes had a 26.8 ms median and 44.5 ms maximum;
- a release CLI PNG-to-WebP workflow completed in 416.1 ms including process startup, two probes, conversion, staging, and publication;
- the 31,670-byte 1176 by 799 PNG fixture produced a verified 1060 by 720 WebP of 10,102 bytes under the shared default recipe;
- the release desktop executable was 12.8 MiB and used about 80 MiB after first preview, rising to about 132 MiB after source and output textures were cached;
- fixed tests passed for Unicode paths, collision increments, input immutability, alpha, orientation, ICC profile, comment strip/preserve, animation rejection, hostile dimensions, undeclared path denial, cancellation cleanup, and post-write properties.

ImageMagick documents that its default model is open unless policy denies operations, so the application policy is mandatory. The accepted limits follow the documented policy domains and are checked in addition to recipe validation. The ImageMagick license permits redistribution with conditions including acknowledgement; no bundled build is accepted by this record. See the official [security policy](https://imagemagick.org/security-policy/) and [license](https://imagemagick.org/license/).

**Consequences and exclusions**

D-013 remains Proposed for video, audio, PDF, document, metadata-only, OCR, and broader image-engine decisions. D-020 selects the RAW product boundary, but no RAW capability is advertised until that adapter passes qualification. D-014 also remains Proposed: development discovery is implemented, but provenance, exact bundled build configuration, updates, acknowledgements, and redistribution are not yet closed. SVG, PDF, network protocols, delegates, arbitrary ImageMagick expressions, arbitrary coder selection, animation transforms, and long-path guarantees are not part of the accepted M2 surface.

### D-019: Frictionless task-first image surface

**Status:** Accepted
**Date:** 2026-07-16
**Related:** D-001, D-003, D-006, D-018

**Context**

The first M2 workbench exposed recipe and engine structure directly through multiple panels, modes, numeric crop bounds, and adjustment groups. Although those controls mapped to typed parameters, the result required more decisions and more navigation than the web utilities the product is intended to replace. Preview regeneration also blanked or flashed the image between changes.

**Decision**

The image product is a task-first surface, not a visible operation graph. Its primary loop is:

```text
Import or drop -> Resize | Crop | Convert -> inspect stable result
               -> Export                         |
               -> Continue with result <---------+
```

For M2:

- only `Resize`, direct-manipulation `Crop`, and `Convert` are top-level tools;
- crop uses a rectangle over the source image with move and corner handles; pixel coordinate fields are not a primary interaction;
- the last complete preview remains painted while a replacement is generated and decoded; a small busy indicator communicates pending work without clearing the canvas;
- repeated edits reuse the source proxy for the current asset;
- `Continue` promotes the exact encoded result to a durable working-cache input, clears applied operations, and begins the next task without another picker;
- Import supports both a dependable native picker and desktop file drop;
- engine modes, recipe terminology, camera-style adjustment groups, padding, and unqualified controls do not appear merely because parameters exist;
- batch selection remains available, but queue chrome appears only when work is actually queued;
- M3 video work does not begin until this loop passes current product review.

The CLI and later web renderer should expose the same task verbs and composable output concept without copying the desktop layout.

**Consequences**

- advanced operation stacking remains an engine capability, not M2 navigation;
- a control must complete a recurring job or materially improve inspection to earn permanent surface area;
- direct manipulation requires tested coordinate mapping across fit, zoom, pan, window resizing, and source dimensions;
- the working cache becomes owned temporary state and must be cleaned safely.

### D-020: Darktable boundary for RAW development

**Status:** Accepted
**Date:** 2026-07-16
**Related:** D-007, D-013, D-014, D-019

**Context**

ImageMagick brightness, contrast, hue, blur, and sharpen flags do not constitute a credible RAW-development pipeline. Presenting them as camera-style controls creates false capability and duplicates work already handled by a mature, color-managed, non-destructive RAW system.

**Decision**

Darktable is the selected engine boundary for future RAW development. The adapter will use `darktable-cli` for headless export and will treat an optional XMP sidecar/history stack as part of the input contract. It may expose reviewed Darktable styles and a small set of qualified export parameters, but it will not recreate Darktable's development modules as approximate ImageMagick sliders.

The official Darktable manual documents headless file and folder export, optional XMP history input, repeated `--import`, size/upscale options, styles, output formats, and ICC output controls. Its sidecar model keeps the original RAW file read-only and records processing history separately.

**Implementation gate**

`darktable-cli` is not installed on the current Windows development machine. No RAW capability is advertised and no RAW controls are rendered until a pinned version passes discovery, representative camera fixtures, XMP round trips, color/profile comparison, cancellation, path isolation, performance, licensing, and distribution review. This decision selects the boundary; it does not claim the adapter is implemented.

References: [darktable-cli manual](https://docs.darktable.org/usermanual/development/en/special-topics/program-invocation/darktable-cli/), [Darktable sidecar model](https://docs.darktable.org/usermanual/development/en/overview/sidecar-files/sidecar/).

## 8. System Architecture

### 8.1 System context

```text
                      +----------------------+
                      |      User files      |
                      | project / temp / out |
                      +----------+-----------+
                                 |
                                 v
+-------------+       +----------+-----------+       +------------------+
| GPUI desktop| ----> | Bunch application   | ----> | Qualified engines|
| or CLI      | <---- | services and runner | <---- | and OS facilities|
+-------------+       +----------+-----------+       +------------------+
                                 ^
                                 |
                      +----------+-----------+
                      | Optional Rust server |
                      +----------+-----------+
                                 ^
                                 |
                      +----------+-----------+
                      | TypeScript + WASM UI |
                      +----------------------+
```

### 8.2 Shared logical pipeline

```text
Input binding
    |
    v
Recipe validation
    |
    v
Capability resolution
    |
    v
Deterministic planning
    |
    v
Executor validation
    |
    v
Staged output directory
    |
    v
Adapter execution and progress
    |
    v
Output probing and verification
    |
    v
Atomic publication or reviewed export
```

### 8.3 Native desktop flow

```text
Drop/select files
      |
      v
GPUI workspace state ---- preview generation ----> proxy cache
      |
      v
Application command
      |
      v
Rust validation and planner
      |
      v
Native job supervisor
      |
      +----> engine adapter ----> child process
      |                              |
      <---------- JobEvent stream ---+
      |
      v
GPUI queue, inspector, diagnostics, artifact review
```

GPUI never owns a child-process handle inside render code. The application service or runner owns process state and publishes snapshots or events.

### 8.4 Self-hosted web flow

```text
TypeScript renderer
      |
      v
Dedicated Worker + bunch-wasm
      |  local validation and advisory plan preview
      v
RecipeRequest over HTTP
      |
      v
Rust server authentication and path policy
      |
      v
Server-side validation and fresh plan
      |
      v
Native job supervisor and adapters
      |
      v
Job events over WebSocket or SSE
      |
      v
Artifact manifest and authorized download
```

The WASM advisory plan is useful for immediate UI feedback, but the server plan is authoritative.

### 8.5 Browser-local flow

Deferred browser-local execution follows the same recipe concepts but may map operations to WebCodecs, Canvas, browser file handles, or WASM libraries.

```text
Recipe -> browser capability check -> browser-local plan -> worker execution
```

An operation supported by the native FFmpeg adapter is not automatically supported by the browser executor. Capability identifiers include the executor and implementation version used for the plan.

## 9. Repository and Crate Plan

The proposed initial layout is:

```text
Cargo.toml
ADR.md

crates/
  bunch-core/
    src/
      asset.rs
      capability.rs
      operation.rs
      recipe.rs
      validation.rs
      output.rs
      error.rs

  bunch-planner/
    src/
      lib.rs
      plan.rs
      naming.rs
      estimates.rs

  bunch-protocol/
    src/
      command.rs
      event.rs
      snapshot.rs
      version.rs

  bunch-native/
    src/
      discovery.rs
      process.rs
      supervisor.rs
      staging.rs
      platform/
      adapters/

  bunch-wasm/
    src/lib.rs

apps/
  desktop/
    src/
      main.rs
      app.rs
      actions.rs
      assets.rs
      theme.rs
      workspace/
      views/
      components/
      preview/

  cli/
    src/main.rs

  server/
    src/main.rs

web/
  app/
    src/
      main.ts
      protocol.ts
      engine.worker.ts
      state/
      views/

fixtures/
  images/
  video/
  pdf/
  documents/

scripts/
docs/
```

### 9.1 Dependency direction

```text
bunch-core <--- bunch-planner <--- bunch-native <--- desktop / CLI / server
     ^               ^                                     |
     |               |                                     |
     +-------- bunch-protocol ------------------------------+
     ^               ^
     |               |
     +----------- bunch-wasm <--- TypeScript
```

The exact graph may be adjusted to avoid cycles, but these rules are binding:

- core does not depend on planner, protocol transport, UI, or executors;
- planner does not depend on GPUI, processes, or browser APIs;
- protocol DTOs do not contain GPUI entities or native process handles;
- native code may depend on portable crates, never the reverse;
- the WASM crate cannot enable native process or filesystem dependencies;
- desktop rendering cannot be imported by CLI, server, or WASM targets.

### 9.2 Avoiding premature crate fragmentation

The layout shows ownership boundaries, not a requirement to create an adapter crate for every engine on day one. Initial adapters may be modules inside `bunch-native`. They should become separate crates only when independent testing, feature gating, licensing, or compile-time weight justifies it.

## 10. Domain Model

### 10.1 Asset

An `Asset` is an inspected input or produced output. It is not only a path.

Conceptual fields:

```text
AssetId
AssetBinding          local path, server object, or browser handle identity
DetectedMediaType     derived from probing, not extension alone
ByteLength
ContentFingerprint    optional until required
MetadataSummary
Dimensions            when applicable
Duration              when applicable
StreamsOrPages        when applicable
ColorAndProfileInfo   when applicable
ProbeProvenance       engine and version
```

Portable recipes should not embed machine-specific absolute paths. A recipe describes input slots; an execution request binds concrete assets to those slots.

### 10.2 Operation

An `Operation` is a typed transformation intent, for example:

```text
ImageResize
ImageCrop
ImageConvert
ImageCompress
ImageComposite
MetadataInspect
MetadataRemove
VideoTrim
VideoConcatenate
VideoInsertStill
VideoExtractFrames
VideoTranscode
GifOptimize
PdfMerge
PdfSplit
PdfRotate
DocumentConvert
```

This list is illustrative and not yet a committed first-release inventory.

Each operation definition includes:

- stable operation identifier;
- schema version;
- accepted input kinds;
- output kind;
- typed parameters and validation constraints;
- whether it changes pixels, timing, metadata, streams, or document structure;
- whether it can be previewed or estimated;
- whether it may be lossy;
- executor capability requirements;
- expected artifact count;
- output naming hints.

### 10.3 Recipe

A recipe is the portable transformation graph.

Conceptual representation:

```json
{
  "schema_version": 1,
  "name": "README image",
  "inputs": [{ "slot": "source", "accepts": ["image/*"] }],
  "steps": [
    {
      "id": "resize",
      "operation": "image.resize",
      "parameters": { "max_width": 1600, "allow_upscale": false }
    },
    {
      "id": "encode",
      "operation": "image.encode",
      "parameters": { "format": "webp", "quality": 82 }
    }
  ],
  "output": {
    "name_template": "{stem}.webp",
    "collision": "increment"
  }
}
```

The final schema must be produced from Rust types and covered by migration and round-trip tests. This example is explanatory, not a frozen schema.

### 10.4 Execution plan

A plan is a resolved, immutable snapshot created for one executor and binding. It may include:

- selected adapter and engine version;
- concrete staged input and output locations;
- normalized arguments;
- intermediate artifacts;
- expected step dependencies;
- resource estimates and configured limits;
- verification probes;
- cleanup actions;
- publication behavior.

Plans are diagnostic artifacts and may be displayed or exported for debugging. They are not accepted as authoritative instructions from remote clients.

### 10.5 Artifact manifest

Successful jobs produce an artifact manifest containing:

- artifact identity and role;
- final path or authorized object identity;
- byte length;
- detected type and key media properties;
- originating recipe and operation chain identity;
- engine provenance;
- warnings, including lossy conversion or discarded streams;
- optional checksum;
- publication or replacement result.

## 11. Capability Model

The UI is driven by executor capabilities rather than a hard-coded assumption that every operation is available.

Conceptual capability identity:

```text
CapabilityId
OperationId
ExecutorKind          native, server, browser
AdapterId
AdapterVersion
EngineName
EngineVersion
InputConstraints
ParameterConstraints
OutputConstraints
PreviewSupport
ProgressSupport
CancellationSupport
SecurityPolicyId
```

### 11.1 Discovery

Native discovery should:

1. Check explicit application configuration.
2. Check bundled, verified sidecars when distribution enables them.
3. Check qualified system locations or PATH when allowed.
4. Run a bounded version probe without shell expansion.
5. Compare the version with the adapter's supported range.
6. Run optional capability probes where build flags affect behavior.
7. Publish a diagnostics snapshot to the application.

Discovery never downloads an executable automatically.

### 11.2 Capability failure

Unavailable operations remain visible only when doing so helps explain a recipe or dependency. They must show a concrete reason, such as:

- engine not found;
- engine version unsupported;
- required codec or delegate absent;
- operation unavailable on this executor;
- server policy disables the operation;
- input exceeds configured limits.

## 12. Native Execution Model

### 12.1 Job supervisor

The native supervisor owns:

- the immutable job plan;
- cancellation state;
- child-process handles and process-group containment;
- staged directories and cleanup;
- event sequencing;
- adapter progress parsers;
- terminal result publication.

It does not own GPUI entities. The GPUI application receives job events through a channel or service API and updates view state on the GPUI context.

### 12.2 Process invocation

Required rules:

- invoke executables directly with argument arrays;
- do not route adapter commands through `cmd.exe`, PowerShell, or a POSIX shell;
- pass filenames as arguments, not interpolated fragments;
- use explicit working directories;
- clear or constrain inherited environment variables where useful;
- capture stdout and stderr separately;
- bound diagnostic buffering;
- hide child console windows in release desktop builds on Windows;
- terminate the entire job process tree on hard cancellation;
- record executable path, resolved version, and exit status in diagnostics.

### 12.3 Staging and publication

1. Validate input bindings and output policy.
2. Create a job-specific staging directory.
3. Write all intermediate and candidate outputs inside staging when possible.
4. Probe and validate candidate outputs.
5. Publish to the selected destination using collision policy.
6. For replacement, create a recoverable backup or use a documented atomic replacement strategy on the same volume.
7. Remove staging after success or cancellation unless diagnostic retention was explicitly requested.

Partial output must never be presented as a successful final artifact.

### 12.4 Progress

Adapters should prefer structured progress channels provided by the engine. If an engine only reports textual progress, its parser is version-aware and tested against recorded samples. Unknown lines may be retained as bounded diagnostics but do not mutate state unpredictably.

### 12.5 Cancellation

Cancellation proceeds in phases:

1. Mark the job `Cancelling` and stop scheduling new steps.
2. Request graceful engine termination where a reliable protocol exists.
3. Wait for a short adapter-defined grace period.
4. Terminate the contained process tree.
5. Drain process output and reap handles.
6. Remove or quarantine incomplete outputs.
7. Emit exactly one `Cancelled` terminal event.

## 13. GPUI Desktop Architecture

### 13.1 First-screen product shape

The desktop opens to the usable workbench, not a marketing or tutorial page. The initial empty state supports file drop, file selection, recent recipes, and engine diagnostics without forcing project setup.

### 13.2 Stable workspace layout

The intended desktop structure is:

```text
+------------------------------------------------------------------+
| App bar: open | command/search | recipe | run | job status       |
+----------------+--------------------------------+----------------+
| Assets / steps | Preview and direct manipulation| Inspector      |
|                |                                | parameters     |
|                |                                | output policy  |
+----------------+--------------------------------+----------------+
| Queue / progress / warnings / artifacts                          |
+------------------------------------------------------------------+
```

The layout may adapt on narrow windows, but controls do not float unpredictably or resize the preview as status text changes.

### 13.3 UI ownership

Proposed desktop modules:

- `AppModel`: window-level navigation and active workspace identity;
- `WorkspaceModel`: assets, recipe draft, selection, and dirty state;
- `CapabilityModel`: discovered engines and available operations;
- `PreviewModel`: generation, proxy identity, view transform, and warnings;
- `JobQueueModel`: immutable job snapshots and actions;
- `SettingsModel`: user preferences and engine configuration;
- view modules for assets, task tools, preview, result statistics, and queue;
- custom elements for crop overlays, frame strips, waveforms, and comparisons.

GPUI actions represent user intent and call application services. Rendering code does not construct process arguments or write files.

### 13.4 Preview policy

Preview work is separate from final execution:

- large inputs use bounded proxy assets;
- proxies are keyed by source identity, relevant parameters, and engine version;
- parameter-only changes reuse the current source proxy;
- a changed parameter increments the preview generation;
- stale preview results are discarded;
- the last complete GPU texture remains visible until the new proxy is decoded and ready to replace it;
- preview quality and final quality are labeled when they differ;
- color profile, rotation, pixel aspect, timing, and alpha handling are included in preview correctness tests where applicable;
- preview generation never alters the original file.

Video preview design is intentionally inactive while M2 remains open.

### 13.5 GPUI component strategy

Use `gpui-component` for established controls such as inputs, menus, dialogs, lists, toggles, tabs, and scrollable content. Use raw GPUI elements for media surfaces where direct rendering, hit testing, or stable geometry requires it.

Do not create custom controls merely to avoid an existing suitable component. Do not force timeline or crop interaction into generic card layouts.

## 14. Web and Server Architecture

### 14.1 WASM facade

`bunch-wasm` should expose coarse typed operations rather than many low-level Rust internals. Candidate responsibilities:

- parse and migrate recipes;
- validate recipe structure and parameter ranges;
- match against a supplied capability snapshot;
- produce an advisory plan summary;
- calculate deterministic output names;
- expose operation metadata for the TypeScript inspector;
- serialize structured validation errors.

The facade must not expose native paths or imply that browser validation grants server authorization.

### 14.2 TypeScript protocol

The protocol includes discriminated command and event types with request or job identities. Because the protocol will be broader than chessengine's, TypeScript definitions should be generated from, or mechanically checked against, Rust schemas rather than maintained independently without a parity gate.

### 14.3 Worker model

- instantiate WASM in a dedicated module worker;
- keep recipe parsing and non-trivial planning off the main thread;
- correlate every response with a request identity;
- ignore responses for superseded generations;
- terminate and recreate a stuck worker when cooperative cancellation cannot be observed during a synchronous WASM call;
- restore the last accepted recipe and capability snapshot after recreation.

### 14.4 Server API shape

Candidate API groups:

```text
GET    /api/v1/capabilities
POST   /api/v1/assets
POST   /api/v1/jobs
GET    /api/v1/jobs/{id}
POST   /api/v1/jobs/{id}/cancel
GET    /api/v1/jobs/{id}/events
GET    /api/v1/artifacts/{id}
DELETE /api/v1/jobs/{id}
```

The exact transport is proposed, not frozen. HTTP handles commands and artifact transfer. WebSocket or server-sent events handle progress. Every request is bounded, authenticated when not loopback-only, and scoped to authorized roots or uploaded job objects.

### 14.5 Hosting security baseline

The hosted application requires:

- restrictive Content Security Policy;
- correct WASM MIME type;
- no inline script requirement;
- authenticated non-loopback APIs;
- CSRF-resistant command endpoints where cookie authentication is used;
- upload and decompression limits;
- job concurrency and storage quotas;
- explicit cleanup and retention policy;
- origin and proxy configuration that cannot expose a loopback-trusting server;
- cross-origin isolation only when a reviewed feature requires shared memory.

## 15. Security and Privacy Model

### 15.1 Trust boundaries

Treat these as untrusted:

- all selected or uploaded file contents;
- filenames and extensions;
- document references and embedded resources;
- metadata values;
- recipe files imported from repositories;
- browser requests and client-side plans;
- external-engine stdout and stderr;
- PATH resolution and user-configured executable locations;
- server-supplied paths outside configured roots.

### 15.2 Primary threats

- command and argument injection;
- path traversal and unsafe output replacement;
- indirect file reads through engine-specific syntax;
- external delegates or network fetches triggered by crafted documents;
- decompression bombs, huge dimensions, excessive frame counts, and deep nesting;
- parser vulnerabilities in codecs and document engines;
- disk exhaustion from proxies or intermediate files;
- runaway CPU or memory use;
- orphaned child processes;
- malicious recipes requesting destructive behavior;
- unauthorized server jobs or artifact access;
- sensitive content leaked through logs, telemetry, thumbnails, or caches.

### 15.3 Required controls

1. Canonicalize and authorize paths at the executor boundary.
2. Use direct process arguments and never a command shell.
3. Maintain per-adapter argument allowlists and parameter validation.
4. Disable engine network access and dangerous delegates where possible.
5. Apply per-job time, memory, file-count, pixel, page, frame, and disk limits.
6. Contain child processes and kill the process tree on cancellation.
7. Write to staging and verify output before publication.
8. Default to new output names rather than replacement.
9. Require an explicit confirmation contract for destructive operations.
10. Keep tool builds pinned and provenance-verifiable when bundled.
11. Record security policy identifiers with engine diagnostics.
12. Do not emit file contents or full sensitive paths in routine logs.
13. Keep telemetry disabled unless a later decision defines an explicit,
    opt-in, content-free policy.
14. Clear uploaded and temporary server artifacts according to documented
    retention rules.

### 15.4 Image-engine policy

The D-018 ImageMagick adapter policy constrains:

- allowed coders and delegates;
- indirect reads;
- network protocols;
- memory, map, disk, area, width, height, frame count, and elapsed time;
- read and write paths for each job;
- formats that require an external delegate.

The static policy denies delegates, filters, indirect reads, unqualified modules and coders, and excessive resources. Each probe, preview, or final execution also receives a generated policy that denies all filesystem paths except the declared input, staging output, and controlled temporary directory. Recipe and post-write validation are additional controls, not substitutes for engine policy. The default engine policy is not assumed safe for untrusted inputs.

### 15.5 Metadata policy

Metadata changes are explicit. Conversion operations declare whether they:

- preserve all supported metadata;
- preserve color and orientation data only;
- strip privacy-sensitive metadata;
- discard unsupported metadata with a warning.

The application does not market a file as sanitized unless a tested metadata policy and post-write probe confirm it.

## 16. Persistence, Caching, and File Safety

### 16.1 Persisted classes

| Data            | Initial storage              | Portability    | Policy                                    |
| --------------- | ---------------------------- | -------------- | ----------------------------------------- |
| Settings        | Versioned structured file    | Machine-local  | Atomic replacement                        |
| Recipes         | Versioned JSON               | Portable       | Human-reviewable                          |
| Recent items    | Settings or small state file | Machine-local  | Paths may be pruned                       |
| Active queue    | Memory                       | None initially | Terminal snapshots only if later required |
| Preview proxies | Cache directory              | Machine-local  | Rebuildable and bounded                   |
| Job staging     | Temporary/job directory      | None           | Removed after terminal state              |
| Durable history | Deferred                     | Machine-local  | SQLite only after requirements exist      |

### 16.2 Schema rules

- every persisted top-level document has a schema version;
- migrations are explicit functions with tests;
- a newer unknown schema is rejected and left byte-for-byte untouched;
- a failed migration preserves the source and reports recovery instructions;
- defaults are applied by schema migration, not by silently rewriting unknown values during rendering;
- recipes include stable operation identifiers, not Rust enum debug strings.

### 16.3 Cache rules

- cache entries are disposable and never the sole copy of a user artifact;
- cache keys include source identity and all correctness-relevant parameters;
- engine or policy version changes invalidate affected entries;
- cache size is bounded and inspectable;
- cache cleanup does not follow symlinks outside the cache root;
- sensitive thumbnails are not shared across users on the server.

### 16.4 Output collision policy

Initial policies:

- `fail`: do not produce a final artifact if the path exists;
- `increment`: add a deterministic numeric suffix;
- `replace`: explicit, recoverable replacement only;
- `choose`: prompt before execution in interactive contexts.

Non-interactive CLI and server requests must resolve `choose` before execution.

## 17. Error and Diagnostics Contract

Errors are structured around user action, not only engine output.

Conceptual error fields:

```text
ErrorCode
Category             validation, capability, input, engine, output, policy
Summary              concise user-facing message
OperationId
StepId
JobId
AdapterId
EngineVersion
Retryability
Source               internal, process exit, timeout, cancellation
DiagnosticDetails    bounded and redactable
SuggestedActions
```

### 17.1 Error requirements

- validation errors identify the parameter and accepted range;
- capability errors identify the missing or incompatible engine feature;
- engine failures retain bounded stderr and exit information;
- timeouts are distinct from user cancellation;
- partial outputs are identified and excluded from successful artifacts;
- the GPUI and web renderers present the same error meaning;
- CLI errors support machine-readable output in addition to human text;
- diagnostic export requires explicit user action and previews included paths.

## 18. Validation Strategy

### 18.1 Test layers

1. **Core unit tests**

- recipe parsing and migration;
- parameter validation;
- output naming and collision rules;
- capability matching;
- job state transitions;
- deterministic serialization.

1. **Planner golden tests**

- fixed recipe plus capability snapshot produces a reviewed plan;
- native and WASM planning agree on portable plan summaries;
- invalid graphs fail without partial plan state.

1. **Adapter unit tests**

- argument construction;
- structured probe parsing;
- progress parsing;
- engine-version handling;
- diagnostic redaction.

1. **Adapter integration tests**

- fixed small fixtures;
- output type, dimensions, duration, pages, streams, and metadata;
- cancellation and timeout cleanup;
- Unicode filenames and paths with spaces;
- deterministic behavior where promised.

1. **Security regression tests**

- traversal attempts;
- output collisions;
- indirect reads;
- huge dimensions or decompression limits;
- malicious recipe values;
- future schema rejection;
- server-root authorization.

1. **GPUI tests and runtime validation**

- action-to-command mapping;
- stale preview rejection;
- queue state and cancellation;
- layout at minimum and common window sizes;
- real launch with engine diagnostics;
- visual inspection of image, frame-strip, and artifact states.

1. **WASM and TypeScript tests**

- WASM compile without native features;
- protocol parity;
- worker startup, request correlation, restart, and restoration;
- native/WASM validation parity;
- production bundle, MIME type, CSP, and hosting headers.

1. **End-to-end workflow tests**

- import -> configure -> preview -> run -> verify -> reveal output;
- batch execution;
- recipe save and replay;
- application restart and settings recovery;
- server job submission and artifact authorization.

### 18.2 Expected native quality gates

The baseline native gates are:

```powershell
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo build --release --workspace --locked
```

For M1, all commands above pass on Rust `1.93.1`. The release GPUI build needs the Windows SDK shader compiler; when it is not discoverable, set `GPUI_FXC_PATH` to the installed `fxc.exe`. A machine-specific SDK path must not be committed as project configuration.

### 18.3 M1 validation evidence

Validated on 2026-07-16:

- `cargo fmt --all -- --check` passed;
- `cargo check --workspace --all-targets --locked` passed;
- `cargo test --workspace --locked` passed with 17 tests: five core, two planner, two protocol, six native, and two desktop-settings tests;
- `cargo clippy --workspace --all-targets --locked -- -D warnings` passed;
- `cargo build --release --workspace --locked` passed with `GPUI_FXC_PATH` pointing to the locally installed Windows SDK shader compiler;
- `bunch-core`, `bunch-planner`, and `bunch-protocol` compiled for `wasm32-unknown-unknown` without GPUI or native process dependencies;
- `bunch-cli demo` emitted 17 ordered JSON events and ended in exactly one `Succeeded` terminal event with a fake artifact manifest;
- the GPUI desktop executable launched as a responsive native window titled `Bunch of Tools` at 1176 by 799 pixels;
- visual inspection confirmed stable asset, preview, inspector, queue, command, and status regions without overlap.

This evidence proves the M1 contracts and shell. It does not qualify a real image, video, PDF, or document engine.

### 18.4 M2 validation evidence

Validated on 2026-07-16:

- `cargo fmt --all -- --check`, workspace check, all-target strict Clippy, and the full release workspace build passed on Rust `1.93.1`;
- `cargo test --workspace --locked` passed 37 tests: seven core, two planner, two protocol, sixteen native, and ten desktop tests;
- the portable core, planner, and protocol crates still compiled for `wasm32-unknown-unknown` without GPUI, ImageMagick, or native process code;
- ImageMagick discovery required version 7 and verified that the Bunch policy was loaded before advertising `image.resize` capability;
- fixed native tests covered policy coder rejection, undeclared path denial, hostile dimensions, animation rejection, proxy preview, Unicode paths, collision increments, orientation, alpha, ICC profiles, comments, metadata strip and preserve, input immutability, child cancellation, and partial cleanup;
- the release CLI produced a verified 1060 by 720 WebP from the 1176 by 799 PNG fixture with ten monotonic events and exactly one successful terminal state;
- the release GPUI executable accepted the same startup image and default recipe, rendered its proxy, published an incremented WebP, selected the output review tab, and displayed matching 1060 by 720 and 10,102-byte artifact properties;
- the native window remained responsive at 1196 by 839 pixels, and visual inspection confirmed stable asset, preview, inspector, queue, command, tab, and status regions without image paint or text overlap;
- no `.partial` output remained after successful, failed, or cancelled tests.

The latest task-first revision additionally passed current workspace all-target check, strict Clippy, formatting, all 37 tests, debug desktop build, and portable WASM-target checks. New tests cover reusable source proxies, exact encoded preview retention, crop selection normalization, crop movement bounds, idempotent linked dimensions, quarter-turn normalization, and settings migration away from implicit white padding. The rebuilt debug window launched and Windows reported it responsive. Native picker visibility, direct crop feel, stable preview appearance, continuation, and batch export remain manual acceptance items and are not inferred from these automated results.

This evidence accepts D-018 as the native engine foundation. It does not close M2. The 2026-07-16 end-to-end product review rejected milestone closure because the implemented GPUI surface did not provide the required flexible controls, operation-driven preview, batch workflow, destination choice, robust image containment, responsive collapsible layout, or desktop visual quality. It also does not accept engine bundling, unqualified image coders, animated transformations, arbitrary ImageMagick commands, or any video, PDF, document, RAW, OCR, server, or browser executor.

### 18.5 Expected web quality gates

Once the web target exists:

```powershell
cargo check -p bunch-wasm --target wasm32-unknown-unknown --locked
cargo test -p bunch-wasm --locked
cargo clippy -p bunch-wasm --target wasm32-unknown-unknown -- -D warnings
npm run check
npm run test
npm run build
npm run test:delivery
```

Exact package names and scripts will be updated when implemented.

### 18.6 Fixture policy

- fixtures are small, license-compatible, and documented;
- original fixture provenance and expected properties are recorded;
- malformed security fixtures are stored or generated safely;
- large performance corpora remain outside normal source control;
- golden outputs are used only when byte stability is meaningful;
- otherwise tests compare probed semantic properties and reviewed tolerances.

## 19. Milestone Plan

### Milestone summary

| Milestone | Outcome                                          | Status      |
| --------- | ------------------------------------------------ | ----------- |
| M0        | Architecture and repository contract             | Complete    |
| M1        | Shared core, protocol, planner, and GPUI shell   | Complete    |
| M2        | First end-to-end image workflow                  | In progress |
| M3        | Video and GIF preparation workflow               | Not started |
| M4        | PDF and document workflow                        | Not started |
| M5        | Recipes, batch work, and CLI parity              | Not started |
| M6        | Native hardening and distributable Windows build | Not started |
| M7        | Self-hosted TypeScript, WASM, and Rust server    | Not started |
| M8        | Browser-local capability subset                  | Deferred    |

### M0: Architecture and repository contract

**Objective**

Establish one reviewed source of truth before scaffolding the application.

**Deliverables**

- this ADR;
- accepted foundational decisions;
- explicit proposed and deferred decisions;
- initial milestone gates;
- initial security and file-safety invariants.

**Exit gates**

- product boundary is accepted;
- native and web renderer boundaries are accepted;
- unresolved engine and server choices remain visibly unresolved;
- no section contradicts the decision index;
- implementation may begin without inventing a second architecture document.

### M1: Shared core, protocol, planner, and GPUI shell

**Objective**

Prove the cross-target architecture before integrating a real processing engine.

**Deliverables**

- Cargo workspace and pinned toolchain policy;
- `bunch-core`, `bunch-planner`, `bunch-protocol`, and `bunch-native` crates;
- versioned recipe draft and migration harness;
- capability and operation descriptors;
- job state machine, event sequencing, and cancellation token;
- fake adapter that emits deterministic progress and artifacts;
- GPUI shell with stable workspace, inspector, and queue geometry;
- CLI command that validates and prints a recipe plan;
- engine diagnostics surface;
- strict formatting, test, and lint gates.

**Exit gates**

- GPUI and CLI submit the same recipe to the same planner;
- fake jobs progress, cancel, fail, and succeed without stale state;
- settings and recipe writes recover from interrupted replacement tests;
- portable crates compile without GPUI and native process dependencies;
- no blocking work runs on the GPUI render path;
- all M1 tests and quality gates pass.

### M2: First end-to-end image workflow

**Objective**

Validate real probing, previews, final execution, safety, and artifact review on the smallest useful media domain.

**Accepted M2 operations**

- inspect dimensions, format, byte size, and projected result;
- resize with linked proportions or an explicit exact size;
- crop through a rectangle overlay;
- rotate and flip;
- convert among the qualified static formats with format-appropriate quality;
- preserve or explicitly remove metadata;
- batch selected inputs;
- compare input and output size and visual result;
- promote an encoded result as the input to the next task.

**Verified engine foundation (not milestone closure)**

- inspect format, dimensions, colorspace, orientation, alpha, frame count, profile names, and byte size;
- generate a bounded PNG proxy with cancellation and stale-generation rejection;
- resize, crop, rotate, flip, fit, fill, and exact-size geometry;
- convert the D-018 qualified static input and output formats;
- select quality and metadata strip or preserve policy;
- publish one selected input through safe staging and incremented collisions;
- compare source and output pixels and bytes using one clipped preview surface with source/output tabs.

This foundation proved the adapter and recipe path, but it is not an acceptable end-to-end product. Fixed output presets, one-input execution, implicit output location, source-only preview behavior, and a rigid desktop shell are temporary implementation constraints that must be removed before M2 can close.

**Deliverables**

- accepted image-engine decision based on a qualification spike;
- restricted engine security policy;
- proxy preview with generation cancellation;
- safe staging and output publication;
- before/after properties and warnings;
- fixed fixtures covering orientation, alpha, profiles, animation, and metadata.
- clear numeric resize controls with linked proportions and exact-size modes;
- a direct crop rectangle with move, corner resize, source bounds, and a deferred expensive render until pointer release;
- qualified common static-image input and output formats with clear capability reporting instead of a hidden three-format limit;
- stable operation-driven result preview and reactive projected output properties without blanking between generations;
- explicit destination-folder selection and deterministic naming;
- dependable native picker import, desktop file drop, multi-select import, batch queueing, and batch export;
- exact encoded-result promotion into a clean next-task input;
- a contained image canvas with fit-to-view reset, pointer-centered `Ctrl+wheel` zoom, click-drag panning, and correct clipping;
- collapsible asset and tool regions with responsive canvas priority across supported desktop window sizes;
- client-side GPUI window chrome and a dark, quiet visual system consistent with the established sibling applications.

**Exit gates**

- input files remain unchanged by default;
- cancellation leaves no published partial artifact;
- output probes match requested dimensions and format;
- metadata behavior matches the selected policy;
- Unicode and collision tests pass;
- resource-limit tests reject hostile dimensions or frame counts;
- a release-ready README image can be produced from GUI and CLI with the same recipe and equivalent artifact properties.
- changing any operation updates the result preview and projected status data;
- the previous complete image remains painted while a new preview is pending;
- crop selection, movement, and corner resizing remain within the source and do not invoke the engine until the interaction completes;
- source and result pixels remain contained at fit and zoomed scales; `Ctrl+wheel` zooms around the pointer, click-drag pans the zoomed image, and fit-to-view restores a fully contained image without incoherent overflow;
- a folder is chosen explicitly before export, and batch output is published only inside that destination;
- native picker cancellation returns to the app without a crash or hidden modal state, and dropped image files enter the same import path;
- `Continue` uses the exact encoded preview artifact, resets applied operations, and produces a valid new source without another file dialog;
- a mixed-format multi-image batch completes with per-item progress, failure, cancellation, collision, and artifact reporting;
- desktop product review confirms readable typography, custom chrome, clear compression and resize semantics, responsive panels, and no layout overlap.

**Initial evidence and reopened status**

D-018 records engine qualification. The fixture and integration tests prove input immutability, cancellation cleanup, format and dimension verification, both metadata policies, Unicode paths, collision behavior, hostile-dimension and animation rejection, and per-command path denial. CLI and GPUI executions of `Recipe::image_resize()` produced the same 1060 by 720, 10,102-byte WebP properties from the README screenshot input.

The milestone closure recorded by the initial implementation was rejected in product review on 2026-07-16. The controls were preset-bound, preview and status were not operation-reactive, image containment was broken, output location was implicit, batch import/export was absent, panel geometry was rigid, and the visual system did not meet the native GPUI standard. M2 remains `In progress` until every exit gate above has current workflow and visual evidence.

The 2026-07-16 task-first revision implements the D-019 surface, helper-process native picker, file drop, reusable source proxies, pre-decoded GPUI textures, direct crop geometry, and exact-result continuation. Automated checks validate the state and engine contracts, but real-window import, crop, continuation, batch export, and visual acceptance still require a fresh product review. M2 is therefore still `In progress`.

### M3: Video and GIF preparation workflow

**Objective**

Cover the recurring recording and short-demo tasks without becoming a nonlinear video editor.

**Candidate operations**

- probe video, audio, subtitle, and attachment streams;
- trim by time range;
- concatenate compatible clips or perform an explicit normalized concat;
- insert or prepend a still cover/title frame for a duration;
- extract selected frames or contact sheets;
- transcode with clear codec, size, frame-rate, and quality choices;
- retain, remove, or replace audio explicitly;
- convert and optimize short GIFs;
- show a frame strip and optional waveform proxy.

**Non-goals for M3**

- arbitrary layered compositing;
- keyframe animation system;
- color-grading suite;
- plugin effects ecosystem;
- full continuous timeline playback if proxy frames are sufficient for the accepted workflows.

**Exit gates**

- frame-accurate expectations are defined for each supported operation;
- progress is parsed and cancellation reaps the full process tree;
- covers, trims, and concatenation preserve or intentionally change streams;
- output duration, dimensions, frame rate, and stream manifest are verified;
- GUI and CLI recipes agree;
- representative short recordings complete within reviewed resource bounds.

### M4: PDF and document workflow

**Objective**

Add bounded document preparation without treating every document format as interchangeable.

**Candidate PDF operations**

- inspect pages, encryption, dimensions, and metadata;
- merge, split, extract, reorder, and rotate pages;
- structural optimization;
- reviewed compression profiles where rasterization consequences are explicit;
- render bounded page thumbnails for preview.

**Candidate document operations**

- Markdown to HTML, DOCX, EPUB, or PDF through qualified pipelines;
- HTML or supported markup to Markdown;
- template and metadata bindings;
- embedded-resource policy;
- cover-image handling where the format supports it.

**Exit gates**

- encrypted or unsupported inputs fail without modification;
- page order and count are verified after every structural operation;
- lossy PDF compression is never described as structural optimization;
- external references and embedded resources follow explicit policy;
- document conversions report unsupported or discarded features;
- output opens in an independent validator or viewer used by the test harness.

### M5: Recipes, batch work, and CLI parity

**Objective**

Turn individual tools into a coherent developer workflow.

**Deliverables**

- save, inspect, migrate, duplicate, and delete recipes;
- bind one recipe to many compatible inputs;
- deterministic output naming and collision preview;
- queue ordering, concurrency limits, and per-job cancellation;
- CLI validate, plan, run, inspect, and capabilities commands;
- machine-readable CLI events;
- project-local recipe discovery by explicit opt-in;
- recipe import review that shows operations and output policy before trust.

**Exit gates**

- the same recipe schema is used everywhere;
- batch failures are isolated and summarized without losing successful outputs;
- concurrency respects resource and disk limits;
- an imported recipe cannot escape selected roots or invoke arbitrary commands;
- GUI and CLI artifact manifests are semantically equivalent.

### M6: Native hardening and distributable Windows build

**Objective**

Convert the development application into a trustworthy Windows distribution.

**Deliverables**

- accepted bundled-versus-discovered decision for each required engine;
- pinned engine builds and provenance records where bundled;
- third-party notices and license compliance review;
- packaged security policies;
- Windows Job Object or equivalent process-tree containment;
- application icon, metadata, GUI subsystem behavior, and diagnostics;
- installer or portable distribution decision;
- update and rollback decision;
- code-signing plan;
- clean-machine and non-administrator tests.

**Exit gates**

- installation does not depend on an undocumented developer environment;
- every bundled binary version and source is traceable;
- offline core workflows pass on a clean supported machine;
- uninstall and cache cleanup behavior are documented and tested;
- resident processes do not prevent updates without a clear recovery path;
- release build passes all security and workflow gates.

### M7: Self-hosted TypeScript, WASM, and Rust server

**Objective**

Expose the shared Rust brain through a private hosted renderer without weakening the native security model.

**Deliverables**

- `bunch-wasm` facade and parity tests;
- TypeScript application with dedicated worker;
- mechanically checked Rust/TypeScript protocol;
- Rust server using the shared core, planner, and native runner;
- accepted upload/server-root file model;
- authentication and authorization policy;
- job progress stream and artifact authorization;
- retention, quotas, and cleanup;
- production CSP, MIME, cache, and proxy configuration;
- responsive actual workbench, not a marketing page.

**Exit gates**

- server rejects client-generated plans and unauthorized paths;
- native and WASM recipe validation parity passes;
- WASM work never blocks the browser rendering thread;
- worker cancellation and restoration pass;
- uploaded and generated files follow retention policy;
- concurrent users cannot read each other's jobs or cache entries;
- production deployment passes delivery and security-header tests.

### M8: Browser-local capability subset

**Status:** Deferred

**Objective**

Run a useful subset without server-side processing when browser APIs and WASM engines can meet correctness, privacy, performance, and bundle-size gates.

**Entry conditions**

- M7 is complete;
- target operations are explicitly selected;
- browser file and memory constraints are measured;
- codec licensing and distribution are reviewed;
- cancellation and worker recovery are proven;
- capability differences can be presented without confusing the user.

## 20. Release and Distribution Policy

### 20.1 Release channels

No release-channel decision is accepted yet. Candidate stages are:

- local development builds;
- signed preview builds for controlled testing;
- stable Windows release;
- separately versioned self-hosted server and web bundle.

### 20.2 Engine provenance

For every bundled engine, release metadata records:

- project name and upstream location;
- exact version or source revision;
- build configuration and enabled features;
- binary checksum;
- license and notice requirements;
- corresponding source availability requirements;
- local patches;
- security policy configuration;
- supported adapter version range.

### 20.3 Compatibility policy

The application version, recipe schema, protocol version, adapter version, and engine version are separate concepts. Compatibility is tested and declared; it is not inferred from one product version number.

## 21. Risks and Mitigations

| Risk                            | Impact                                   | Mitigation                                                              |
| ------------------------------- | ---------------------------------------- | ----------------------------------------------------------------------- |
| GPUI breaking changes           | Desktop churn                            | Pin exact version; isolate GPUI types; upgrade through explicit gate    |
| UI monolith                     | Slow changes and regressions             | Split state and views from M1; keep services outside rendering          |
| External-engine vulnerabilities | Host compromise or data exposure         | Pin, update, restrict policies, contain processes, limit resources      |
| Licensing mistakes              | Distribution blocked                     | Qualify exact builds; retain provenance and notices before bundling     |
| Format scope explosion          | Product never becomes coherent           | Milestone-specific operation inventories and non-goals                  |
| Silent quality loss             | User distrust and damaged artifacts      | Explicit lossy warnings; independent output probes; comparison UI       |
| Inconsistent GUI and CLI        | Recipes become unreliable                | One Rust planner and shared acceptance fixtures                         |
| Native and WASM drift           | Web becomes a second product             | Parity tests and generated/checked protocol types                       |
| Browser main-thread stalls      | Unusable web interface                   | Dedicated workers and hard worker restart                               |
| Server command injection        | Remote code execution                    | Recipe-only API, fresh server plans, direct argv, strict adapters       |
| Disk exhaustion                 | Failed jobs or host instability          | Estimates, quotas, staged cleanup, bounded caches                       |
| Orphaned engines                | Resource leaks and file locks            | Process-tree containment and supervisor reaping                         |
| Incorrect cancellation          | Partial output presented as valid        | Terminal-state invariant and output verification                        |
| PATH engine drift               | Reproducibility failure                  | Version probes, supported ranges, diagnostics, eventual pinned bundles  |
| Preview/final mismatch          | Wrong user decisions                     | Include relevant parameters and engine versions in proxy keys and tests |
| Remote file ambiguity           | Unauthorized access or confusing uploads | Explicit server-root and upload bindings; no arbitrary client paths     |

## 22. Open Questions

These are unresolved and must not be treated as accepted implementation facts.

### Product and interaction

- Is `Bunch of Tools` the release name or only the repository name?
- Which exact three workflows define the first public usefulness threshold?
- Should the default interaction begin with file selection, operation search, or remembered project recipes?
- How should advanced engine controls be exposed without leaking raw CLI syntax?
- Should project-local recipes use a conventional directory such as `.bunch/`?

### Native engines

- ImageMagick or libvips for the first image slice?
- Which FFmpeg build configuration and codecs are acceptable for distribution?
- Which engine renders PDF thumbnails?
- What is the acceptable RAW workflow boundary?
- Is OCR part of the product or an optional adapter family?
- Which engines can be safely discovered from PATH in a supported release?

### Preview and editing

- Which video preview backend provides reliable Windows playback inside GPUI?
- Is a frame-strip editor sufficient before continuous playback?
- How are color-managed image previews validated across GPU and engine output?
- What maximum proxy size and cache policy keeps previews responsive?

### Persistence and history

- When does durable job recovery justify SQLite?
- Are artifact manifests saved beside outputs, in application state, or only on explicit request?
- How long should diagnostics and preview caches remain by default?

### Server and web

- Uploads, configured server roots, or both?
- Single-user loopback service first, or authenticated LAN deployment?
- WebSocket or server-sent events for job progress?
- Which operations are valuable enough for browser-local execution?
- Is server-side project-root discovery appropriate, or should all access be explicit per deployment?

### Distribution

- Portable archive or installer?
- Automatic updates or manual signed releases?
- Code-signing certificate and release identity?
- Open-source, source-available, or proprietary product license?

## 23. Rejected Alternatives

### R-001: Electron desktop shell

**Status:** Rejected

The accepted desktop direction is Rust plus GPUI. Shipping a browser runtime is not justified for the native product, and it would not establish the desired Rust-native UI and execution boundary.

### R-002: Tauri desktop shell

**Status:** Rejected

Tauri would provide a smaller webview shell than Electron, but it still makes HTML and TypeScript the native renderer. The accepted requirement is GPUI with no webview. Tauri remains irrelevant unless that requirement is superseded.

### R-003: Python as the application core

**Status:** Rejected

Python is effective for experiments but would complicate a compact native distribution, GPUI integration, cross-target WASM reuse, and strict process and type boundaries. Python-based external tools may still be qualified as adapters.

### R-004: Reimplement codecs and document formats in Rust

**Status:** Rejected

The project would spend its effort reproducing mature format engines rather than solving workflow, safety, reproducibility, and interaction problems. Focused Rust libraries remain valid when they are mature and reduce external-process complexity for a bounded operation.

### R-005: One renderer shared between desktop and web

**Status:** Rejected

GPUI is the desktop renderer and TypeScript is the web renderer. Attempting to force presentation reuse would weaken both. The reusable contract is the Rust domain, planner, protocol, and behavior tests.

### R-006: Arbitrary shell-command recipes

**Status:** Rejected

They would defeat validation, capability discovery, server security, portable recipes, predictable previews, and meaningful error handling. Advanced usage belongs in typed adapter capabilities or an explicitly separate developer tool.

### R-007: Cloud-first processing

**Status:** Rejected

It conflicts with the primary privacy and offline goals. A self-hosted server is an optional later executor, not the foundation of desktop usefulness.

### R-008: Promise immediate native/browser parity

**Status:** Rejected

Native processes and browser sandboxes have materially different capabilities. Parity applies to shared recipe semantics and validation where supported, not to the availability or performance of every operation.

## 24. Chronological Decision Log

### 2026-07-16: Product problem established

The project began from recurring friction around converting, compressing, editing, composing, and preparing images, GIFs, recordings, videos, PDFs, Markdown, and other project files. Privacy and workflow interruption were the primary motivations, not creation of another generic converter website.

### 2026-07-16: Initial stack direction considered

A hybrid desktop shell with Rust orchestration and mature external processing engines was considered. Tauri and a TypeScript desktop renderer were initially discussed as a practical default.

### 2026-07-16: Native renderer corrected to GPUI

The native direction was clarified as Rust plus GPUI with no webview. The later web surface was clarified as a separate TypeScript renderer using a WASM export of the shared Rust brain.

### 2026-07-16: Existing GPUI projects inspected

Woodpecker, DevHub GPUI, and chessengine were reviewed for local precedent. Their useful patterns were adopted:

- Woodpecker: native execution boundary, snapshots, channels, coalesced atomic persistence, and exact GPUI pins;
- DevHub GPUI: separate core crate, cancellation tokens, generations, platform isolation, and versioned crash-safe state;
- chessengine: shared Rust library, native/web feature gates, GPUI native host, `wasm-bindgen` adapter, typed TypeScript worker protocol, and parity gates.

The concentration of UI state into large files was identified as a pattern not to repeat in the broader Bunch of Tools surface.

### 2026-07-16: Foundational architecture accepted

The product accepted one shared Rust brain, GPUI desktop renderer, TypeScript web renderer, WASM facade, target-specific executors, typed recipes, narrow tool adapters, and server-side revalidation. Exact engine selection, bundling, browser-local execution, and remote file access remain proposed or deferred.

### 2026-07-16: ADR-000 created

This document became the canonical architecture, decision, milestone, risk, and validation ledger. It was accepted as the basis for implementation, completing M0.

### 2026-07-16: M1 shared contract and native shell completed

The pinned Rust workspace now separates portable domain, planning, and protocol crates from native execution and both native front ends. Versioned recipes, capabilities, deterministic planning, sequenced job events, cancellation, crash-safe persistence, diagnostics, CLI JSON output, and the GPUI workbench are implemented around the explicit `foundation.simulate` harness.

All native format, check, test, lint, and release-build gates passed. The three portable crates also compiled for `wasm32-unknown-unknown`. A CLI lifecycle smoke ended in one successful terminal event, and a real GPUI launch plus visual check confirmed the desktop layout. No real processing engine was selected or implied; at the M1 checkpoint, D-013 was Proposed as the first M2 gate.

### 2026-07-16: Initial M2 native engine slice implemented

ImageMagick 7 was qualified and accepted only for the narrow D-018 `image.resize` adapter. The portable recipe gained typed format, geometry, upscaling, quality, and metadata parameters. Native discovery, structured probe, per-command security policy, proxy generation, child cancellation, staged publication, collision increments, post-write verification, and before/after artifact properties now serve both CLI and GPUI.

The first dual-image GPUI comparison exposed unreliable paint containment in the capture path. The product surface was tightened to one clipped image stage with source/output tabs and a separate property comparison; the repeated real-window capture then showed no overlap. All 29 tests, strict lint, release build, WASM portability check, real CLI workflow, and real GPUI export passed. This proved the native engine slice; D-013 remains Proposed for later video and GIF engine qualification, and D-014 remains Proposed for bundled-engine distribution.

### 2026-07-16: M2 product closure rejected and milestone reopened

End-to-end review found the shipped slice insufficient as an image workbench. Preset-only operations prevented arbitrary sizing and adjustment; source and result rendering were not reliably contained; the preview, output properties, and status bar did not respond coherently to recipe changes; export did not ask for a destination; and batch import and export were absent. The rigid, non-collapsible layout, tiny typography, ambiguous resize/compression language, and generic native frame also failed the established GPUI desktop design bar.

M2 is therefore `In progress`. The D-018 engine qualification remains valid, but engine success cannot substitute for workflow or product acceptance. M3 is held until flexible operations, broader qualified formats, live result preview, explicit batch export, correct canvas behavior, collapsible responsive panels, custom chrome, and visual review all pass the revised M2 exit gates.

### 2026-07-16: M2 narrowed to a frictionless image loop

Product review rejected the inspector-style workbench and approximate camera-adjustment surface. D-019 now binds the desktop to Import/Drop, Resize, direct Crop, Convert, Export, and Continue. Numeric crop bounds and the visible color-adjustment group were removed. Result previews retain the previous GPU texture while a replacement is processed and decoded, and repeated edits reuse the current source proxy.

The Windows picker was moved into a short-lived helper invocation of the same executable. This prevents the native modal loop from re-entering GPUI and avoids the COM apartment failure observed when a synchronous or asynchronous `rfd` dialog ran inside the GPUI process. Desktop file drop now shares the import path. The exact encoded preview is retained long enough for `Continue` to promote it into an owned working-cache input and reset the operation state.

D-020 selects Darktable as the future RAW boundary and explicitly rejects ImageMagick adjustment flags as a RAW workflow. No capability is advertised: `darktable-cli` is absent on the current machine and qualification remains an implementation gate. M3 stays closed and M2 remains `In progress` pending a fresh real-window workflow and visual review.

## 25. Decision Record Template

Use this template for future material decisions:

```markdown
### D-XXX: Short decision title

**Status:** Proposed | Accepted | Deferred | Rejected | Superseded  
**Date:** YYYY-MM-DD  
**Supersedes:** D-XXX, if applicable

**Context**

What changed or what problem requires a decision?

**Decision**

What is binding?

**Rationale and evidence**

Why, with commands, measurements, compatibility results, or accepted product
constraints where relevant?

**Consequences**

What becomes easier, harder, required, or intentionally unsupported?

**Revisit trigger**

What concrete evidence would justify reopening this decision?
```

## 26. Definition of Done for Architectural Changes

An architectural change is done only when:

- its decision status and rationale are recorded;
- affected diagrams and dependency rules are updated;
- security and privacy consequences are reviewed;
- persistence or protocol compatibility is addressed;
- affected milestones and risks are updated;
- validation gates exist and have passed when implementation is claimed;
- the chronological log records the change;
- superseded behavior remains discoverable in this document.
