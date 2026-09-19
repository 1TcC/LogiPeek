# LogiPeek contributor and agent rules

## Scope and isolation
- Build a lightweight, unofficial Windows tool in Rust for Logitech mouse battery and DPI capabilities.
- The only permitted project working directory is `C:\Users\96174\Desktop\LogiPeek`. Keep all project reads, writes, searches, Git operations, temporary files, tests, and build outputs inside it.
- Do not actively read or modify other projects or user files. Do not change global Git, Codex, Windows, registry, authentication, or application settings unless explicitly authorized by the user. The installation exception below permits only normal installer registration and the required user PATH/RUSTUP_HOME/CARGO_HOME settings.
- The user's authorization dated 2026-09-19 permits installing only the following for LogiPeek: Rustup stable for `x86_64-pc-windows-msvc`, Cargo, `rustc`, `rustfmt`, Clippy, Visual Studio 2022 Build Tools, the MSVC toolset, the Windows SDK, and dependencies strictly necessary for those components. This authorization does not permit any other system modification or software installation.
- Prefer `D:\DevTools` for every customizable installation location, toolchain location, package/cache location, and build/dependency cache. Use the C: drive only for installation metadata or system components that cannot reasonably be relocated or customized. Keep project files, temporary files, tests, and build outputs in `C:\Users\96174\Desktop\LogiPeek`.
- Do not install unrelated tools or runtimes, including Node.js, Python, CMake, Ninja, LLVM, MinGW, WSL, Docker, IDEs, or similar utilities. Do not change or disable security mechanisms. Do not manually modify tool caches.
- If an authorized dependency is unavailable, install only the minimum necessary component within these rules; otherwise stop the affected step and report the missing dependency. No tools beyond this specific authorization or unrelated packages may be installed.
- Only repository-local Git configuration may be changed when necessary.

## Product and protocol
- Use Rust, Windows native APIs, and a small HID abstraction. No Web UI, Electron, Node.js, Chromium, WebView, Tauri, database, background service, or unnecessary async runtime.
- No accounts, telemetry, analytics, uploads, cloud, or runtime network requests. Normal operation should not require Administrator or Logitech software.
- Discover capabilities through HID++; never hardcode dynamic feature indices or bind the design to one mouse model.
- Distinguish receivers, their paired device indices, and multiple HID interfaces. Design for multiple devices and partial capability support.
- Treat hardware input as untrusted. Handle disconnects, sleep, timeouts, malformed reports, unsupported features, permissions, and unexpected device indices with explicit errors rather than panics.
- Keep exact battery percentages separate from coarse levels. Never invent percentages or supported DPI values.
- Do not copy GPL source into this MIT project. Prefer public Logitech protocol documentation and independent implementation.
- Keep diagnostics safe to share: no usernames, home paths, serial numbers, MAC/IP addresses, or full HID paths.
- Do not claim hardware compatibility or validation without actual physical testing. Clearly separate implemented features from verified behavior.
- Avoid busy polling. LogiPeek uses one-shot CLI queries.
- The current phase excludes GUI, tray, DPI writes/presets, startup registration, auto updates, RGB, macros, remapping, and onboard profile changes.

## Collaboration
- Use a small number of independent sub-agents where useful; do not delegate merely for the sake of delegation.
- Prefer gpt-5.6-luna with low reasoning for narrow, explicit, mechanical tasks and simple tests/helpers.
- Prefer gpt-5.6-terra with low/medium reasoning for scans, reviews, documentation, and implementation under an established design.
- The main agent owns architecture, HID/HID++ protocols, receiver/multi-device design, battery/DPI semantics, public APIs, hardware safety, integration, and final review.
- Assign explicit file ownership. Agents must never edit the same file concurrently.
- The main agent must review every sub-agent change. Do not claim delegation that did not occur.
- Only the main agent may stage, commit, or push. Sub-agents must not change remotes, branches, Git configuration, or history. Never force push.

## Validation and delivery
- Run appropriate tests after each round of changes; unit tests must not require hardware. Keep hardware tests separate.
- Before delivery run: cargo fmt --check; cargo check; cargo test; cargo clippy --all-targets --all-features -- -D warnings; cargo build --release.
- Run --devices and --diag on available hardware; run --battery only if implemented reliably. Record actual results without exaggeration.
- Review errors, public APIs, unwrap/expect, panic/todo/unimplemented, unsafe, TODO/FIXME, tests, dependencies, diagnostics, and all diffs before committing.
- Maintain matching English and Simplified Chinese READMEs, truthful current architecture/protocol docs, and an MIT license.
- Exclude build outputs, temporary files, secrets, and unrelated machine data from Git. Review staged content before committing.
- The requested delivery branch is main at https://github.com/1TcC/LogiPeek.git. Commit/push only after development, review, and validation; if authentication fails, preserve the local commit without changing system authentication.
- Stop after the authorized phase. Report blockers and unrun checks honestly.
