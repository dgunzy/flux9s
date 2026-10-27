# flux9s Development Instructions

## Core Philosophy

- **Zero-maintenance model updates**: Use code generation over manual types
- **Non-blocking operations**: Keep UI responsive with async task spawning
- **Graceful degradation**: Log errors, don't crash (only panic for truly unrecoverable errors)
- **K9s alignment**: Follow K9s conventions for keybindings and UX

## Before You Start

1. Run `just ci` after all changes (formats, lints, tests)
2. Large changes require accompanying tests
3. Keys and commands are data: add a key to the action table (`src/tui/app/actions.rs`)
   and a command to `COMMAND_TABLE` (`src/tui/app/command_mode.rs`) — dispatch,
   footer, and help all come from those entries. Never add a key arm elsewhere
4. **Git operations**: User manages their own git workflow. Only use git to restore a single file when explicitly requested (e.g., `git restore path/to/file`). Never commit, push, create branches, or perform any other git operations

## Code Standards

### Error Handling

```rust
// ✅ Good - Add context to errors
api.patch(name, &params, &patch)
    .await
    .context(format!("Failed to suspend {}/{}", resource_type, name))?;

// ✅ Good - Log errors, return Result
if let Err(e) = operation() {
    tracing::warn!("Operation failed: {}", e);
    return Ok(()); // Continue execution
}

// ❌ Bad - Missing context
api.patch(name, &params, &patch).await?;

// ❌ Bad - Unwrap (use only in tests)
let value = result.unwrap();
```

### State Management

- Use `Arc<RwLock<>>` only when truly shared across threads
- Prefer simple `HashMap`/`Vec` for single-threaded state
- Keep `App` struct focused - extract complex state into sub-structs

### Async Operations

```rust
// ✅ Good - Spawn tasks, use channels for results
let (tx, rx) = tokio::sync::oneshot::channel();
self.pending_rx = Some(rx);
tokio::spawn(async move {
    let result = operation().await;
    let _ = tx.send(result);
});

// Later, poll for results
if let Some(result) = app.try_get_result() {
    app.handle_result(result);
}
```

### Type Safety

- Resource identity is `ResourceKey` (`kind`, `namespace`, `name`); its string form is
  `to_key_string()` and the only parser is `ResourceKey::parse` (names may contain `:`).
  Never `format!("{}:{}:{}")` or split a key by hand
- API coordinates are kube's `ApiResource` (`FluxResourceKind::api_resource`,
  `KindSpec::api_resource`, `Gvk::api_resource`), never a `(group, version, plural)` tuple

```rust
// ✅ Good - Use proper types
struct ResourceKey {
    resource_type: String,
    namespace: String,
    name: String,
}

// ❌ Bad - Tuple types or string parsing
type Request = (String, String, String); // What do these mean?
let parts: Vec<&str> = key.split(':').collect(); // Fragile
```

### Adding New Resource Types

1. Ensure CRD is in `crds/` directory
2. Run `./scripts/update-flux.sh` to regenerate models
3. Add one row to the `flux_kinds!` table (`src/models/flux_resource_kind.rs`):
   variant (spelled as the CRD `kind`), generated model, and `:` aliases (name, plural,
   short forms). The enum, `FluxResourceKind::ALL`, parsing, aliases/help, and the API
   group/version/plural (`api_resource()`, read from the model's `#[kube(...)]`) all come
   from it; the watcher watches every kind in `ALL`
4. Fill in the kind's arms in the exhaustive `FluxResourceKind` matches the compiler flags
   (`older_api_versions`, `supports_suspend`, `owner_label_prefix`, columns, …)
5. Add its row to `every_kind_reads_its_api_from_the_generated_model` (`tests/model_compatibility.rs`)

When a CRD update promotes a kind (e.g. `v1beta3` → `v1`), that test fails: update it and
add the old version to `older_api_versions`, so clusters still serving it keep working.

**Important**: Always use `FluxResourceKind` (`api_resource()` / `api_resources()`,
capability methods like `supports_suspend()` / `can_orphan()`, `labels::*`) and
`kube::api::api_resource_for_kind()` instead of hardcoding resource types, API groups, versions,
plural names, or Flux labels. Never compare a kind name against a string literal.

### Adding New Operations

1. Implement `FluxOperation` trait
2. Register in `OperationRegistry::new()`
3. Add an `Action` with `ActionSpec::Operation(key)` to `ACTIONS` (`src/tui/app/actions.rs`)
   — its `footer` and `help` fields feed the footer and help overlay — and add its id
   to the footer layouts where it should show
4. Write operation tests

### Adding a Command

`:` commands are dispatched from a data-driven table, not a long `if`/`match` chain.

1. Add a focused `cmd_<name>(&mut self, cmd: &str)` handler on `App` (`src/tui/app/command_mode.rs`)
2. Add a predicate in `src/tui/commands.rs` (e.g. `is_<name>_command`)
3. Register a `CommandSpec { matches, run, help }` in `COMMAND_TABLE` (`command_mode.rs`);
   its `help` lines appear in the help overlay automatically
4. Update the docs site
5. Write a test that drives `execute_command()` (see `src/tui/app/tests.rs`)

Special commands handled directly in `execute_command` (help/quit/readonly and the
connection-error gate) and the resource-type fallback stay outside the table; their
help is `SPECIAL_COMMAND_HELP`.

### Adding a Key (Action)

Every key of the normal key handler is an `Action` in `ACTIONS`
(`src/tui/app/actions.rs`): keys, `when` (view / capabilities — deny by default),
`spec` (`Builtin`, `Operation`, or the data-only write `Patch`, which
share the kind filter, read-only gate, and confirmation via `App::request_write`),
footer hint, help line. Prefer `Patch` over new Rust for simple writes. Table order is
precedence: put view-specific actions above the general ones they override. Footers
are per-view layouts of action ids filtered by `when`, so a footer can't show a key
that doesn't work. `every_footer_hint_runs_the_action_it_advertises` fails if an
earlier action shadows a key a footer advertises.

Popups: each popup id lives in `submenu_ids` (`src/tui/commands.rs`); its builder
uses the id and its selection handler is registered in `SUBMENU_TABLE`
(`src/tui/app/submenus.rs`).

### View Components

- Keep views stateless - they receive all needed data as parameters, grouped: the shared
  groups in `views::props` (`Fetched` = data + loading, `TextScroll` = offset + `/` search,
  `ListCursor` = selection + offset, `Watched` = selected key + state + objects), or a
  per-view `…Props` struct. `too_many_arguments` is not allowed, so a long positional list
  (where two `bool`s or `&mut usize`s can be swapped silently) doesn't compile
- Extract complex rendering logic into helper functions
- Use theme colors consistently (never hardcode colors)
- Handle small terminal sizes gracefully
- Submenu overlays render as centered popups on top of the current view
- State management for submenus lives in `ViewState.submenu_state`
- Event handling priority: confirmation → submenu → filter → normal commands
  (`src/tui/app/keys.rs`; normal keys dispatch through the action table)
- Per-view behavior is consolidated on `impl View` (`src/tui/app/state.rs`):
  `scroll_offset_mut`, `is_list_view`, `is_text_search_view`, `is_nested_view`,
  `footer_layout` (the view's footer layout in `src/tui/app/actions.rs`).
  Add a new view's scroll/back/classification there instead of scattering
  `match current_view` arms across the event handlers.
- **Scrolling is one mechanism.** Declare how a view scrolls in the exhaustive
  `View::scroll_model` (`Offset` / `Selection` / `Focus` / `Fixed`; key handling in
  `App::scroll_by` follows it). Render with `views::scroll::Viewport` — `text` for
  line offsets, `list` for selections — draw only `viewport.range()`, then call
  `viewport.render_scrollbar(f, area, theme)` on the bordered area. Never clamp
  offsets by hand or draw a custom indicator.
- **Long text wraps through one helper.** Messages that can outgrow the screen
  (confirmations, status lines, empty/loading states, anything built from cluster
  data) render with `views::text::wrapped`; a layout that must make room for them
  sizes it with `views::text::wrapped_height` (same word-wrap, so the space matches).
- **Classify views with exhaustive `impl View` predicates** (`is_flux_list`,
  `needs_native`, `is_list_view`, …) — never with an ad-hoc `matches!(view, View::A | View::B)`
  at the call site, and no `_ =>` arm in a `match` on `View`: a new view must be placed in
  every one. Tests that cover "every view" iterate `View::ALL` (generated by `views!`).
- **Back navigation is one stack** (`src/tui/app/navigation.rs`): open views with
  `navigate_to` (drill in) or `navigate_root` (`:` top-level switch), go back with
  `navigate_back`. Never add a per-view "back target" field or assign
  `current_view` directly for a user-visible transition. What a view holds
  (streams, watches, fetches) is released in `leave_view`.

## Testing Requirements

### What to Test

- ✅ CRD compatibility (status field extraction)
- ✅ Resource registry completeness
- ✅ Field extraction from resources
- ✅ Operation validity for resource types
- ❌ Don't over-test generated code (trust kopium)

### Test Organization

```rust
// Unit tests in module
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_behavior() {
        // Test logic
    }
}

// Integration tests in tests/ directory
// tests/crd_compatibility.rs
// tests/resource_registry.rs
```

## Common Patterns

### Shared readers

- `status.conditions`: `crate::kube::conditions` (`find`, `is_true`, `is_ready`, `Condition`)
- Ages: `crate::age::compact` / `crate::age::since`

### Resource Key Format

- Always use `resource_type:namespace:name` format
- Consider creating `ResourceKey` type for type safety

### Channel Management

- Use `oneshot::channel()` for single-response operations
- Store `Receiver` in `App`, spawn task with `Sender`
- Poll with `try_recv()` in main loop (non-blocking)

### Theme Usage

```rust
// ✅ Good - Use theme methods
let style = self.theme.status_ready_style();
let text = Span::styled("Ready", style);

// ❌ Bad - Hardcode colors
let text = Span::styled("Ready", Style::default().fg(Color::Green));
```

### Submenu Implementation

Commands can provide interactive selection menus using the `CommandSubmenu` trait. The implementation involves:

1. **Create a submenu provider** - Implement `CommandSubmenu` trait with `get_submenu()` method
2. **Build submenu items** - Create `SubmenuItem` instances with display text and values
3. **Create submenu state** - Build `SubmenuState` with items, title, and help text
4. **Register in handler** - Add to `get_command_submenu()` function in `src/tui/commands.rs`

**When to use submenus:**
- Command has multiple valid options to choose from (contexts, namespaces, etc.)
- Users may not know exact option names
- Provides better UX than listing options in status message

**Reference:** See `ContextSubmenu` in `src/tui/commands.rs` for implementation example

## Crate layout: one module tree

- **`src/lib.rs` owns every module. `src/main.rs` is a thin shim** that does
  `use flux9s::{cli, config, kube, tui};` and nothing else. Never add `mod <name>;`
  to `main.rs` — re-declaring library modules there compiles the whole codebase a
  second time into a parallel tree that clippy and the test runner never see.
- `cli` is `#[doc(hidden)] #[cfg(feature = "tui")] pub mod` — in the library so it is
  linted and unit-tested, hidden because it is not stable public API, and gated
  because it reaches into `config::ThemeLoader`.
- The `[[bin]]` declares `required-features = ["tui"]`, so
  `cargo check --no-default-features` skips it and actually verifies the headless
  library mode advertised in `lib.rs`. `just check-headless` runs this; it is part of
  `just ci`.
- Test invocations only reach what they name: `cargo test --lib --tests` runs neither
  bin unit tests nor doctests. `just test` therefore also runs `cargo test --doc`.

## CI/CD

- **The justfile is the single source of truth for CI.** `.github/workflows/ci.yml`
  installs `just` and runs one step: `just ci-check`. Never add a check to the
  workflow directly — add it to the `verify` recipe, and both local and CI runs
  pick it up. The two entry points differ in exactly one way:
  - `just ci` = `fmt` + `verify` (formats in place — for local use)
  - `just ci-check` = `fmt-check` + `verify` (unformatted code fails — for CI)

  `verify` = `clippy`, `check-headless`, `audit`, `test`. The `build`, `msrv`, and
  `build-windows` jobs stay in the workflow: they are per-target builds, not
  duplicated verification logic.
- **Test recipes never enumerate targets.** `just test` is `cargo test --tests`
  (lib + bin unit tests + every integration binary, so a new test file is picked up
  automatically) plus `cargo test --doc`, since no target selector covers doctests.
- All CI checks must pass before merge
- Clippy warnings are treated as errors
- Generated code in `src/models/_generated/` has clippy suppressed
- **Lint levels live in `Cargo.toml`'s `[lints.rust]` / `[lints.clippy]` tables**, not
  in the `cargo clippy` command line, so CI, `just clippy`, and rust-analyzer all
  agree. Both invocations are just `cargo clippy --all-targets -- -D warnings`.
  Add or relax a lint in `Cargo.toml` — never by appending `-A` flags.
- **Clippy runs with `--all-targets`.** `src/cli/` and `src/main.rs` belong to the
  binary target only; the old `--lib --tests` invocation never linted them.
- **`unwrap()`, `expect()`, and `panic!` are denied outside tests** (`unwrap_used`,
  `expect_used`, `panic`). `clippy.toml` exempts test code, where an unwrap *is*
  the assertion. Three deliberate exceptions carry a module-level `#![allow]` with a
  written justification: `src/models/extra_kinds.rs` and `src/models/kinds.rs` (a
  poisoned registry/catalog lock is unrecoverable) and `tests/live_tests.rs`
  (`allow-panic-in-tests` does not cover helpers outside `#[test]` fns). Anything
  new needs a real justification, not an allow — degrade gracefully instead.
- **No `#[allow(dead_code)]` / `#[allow(unused_imports)]`.** Everything is `pub`
  from the library crate, so rustc never reports these lints there — the allows
  were pure noise and hid genuinely dead code. Delete unused code instead of
  "reserving" it.

## Supply chain & dependency pinning

- **All GitHub Actions are SHA-pinned** with a trailing `# version` comment
  (OpenSSF Scorecard *Pinned-Dependencies*). When adding an action, pin the full
  commit SHA — never a moving `@v4`/`@main` tag. Resolve it with:
  `gh api repos/<owner>/<repo>/git/refs/tags/<tag> --jq '.object.sha'`.
- **Dependabot moves the pins for us**: `github-actions` monthly, `cargo` weekly
  (`.github/dependabot.yml`). Patch/minor updates auto-merge once CI + cargo-deny
  + CodeQL pass (`dependabot-auto-merge.yaml`); majors are left open for review.
- **Branch-ref pins are the exception Dependabot can't track.** Actions that
  publish no releases and are pinned to a default-branch SHA — currently
  `dtolnay/rust-toolchain@<sha> # master` (the `toolchain:` input selects the Rust
  version) — must be **re-pinned by hand about every month or two**:
  ```bash
  sha=$(gh api repos/dtolnay/rust-toolchain/commits/master --jq '.sha')
  # then bump every dtolnay/rust-toolchain@<old> to @$sha across .github/workflows/
  ```
  Open a PR so CI validates the bump before it lands.
- **Docs-site toolchain pins are also hand-managed.** Two more things Dependabot
  cannot track, both in `.github/workflows/docs.yml` / `docs/go.mod`:
  - `hugo-version: '0.166.0'` — an action *input*, invisible to Dependabot.
  - `require github.com/docsy/docsy/theme vX.Y.Z` in `docs/go.mod` — the Hugo
    theme is referenced only from `docs/config.toml`, so **`go mod tidy` (and
    `hugo mod tidy`) delete the require line** and Hugo then floats the theme
    to its latest release. Never run either in `docs/`; builds set
    `GOFLAGS=-mod=readonly` so a lost pin fails loudly instead of silently
    upgrading.

  Bump the two together (docsy documents its supported Hugo versions per
  release) and let the docs workflow validate it in a PR. Upgrading docsy:
  `hugo mod get github.com/docsy/docsy/theme@vX.Y.Z`, then
  `hugo mod npm pack && npm install` (the theme's Bootstrap / Font Awesome come
  from npm via the generated `docs/packages/hugoautogen` workspace), and read
  that release's upgrade guide.
- **Build the docs with `docs/scripts/hugo.sh`** (after `npm ci` in `docs/`), locally
  and in CI. Docsy 0.17+ compiles SCSS with Dart Sass, pinned as the
  `sass-embedded` npm dev dependency (so Dependabot tracks it); the wrapper puts
  that native binary on PATH and sets `GOFLAGS=-mod=readonly`. Don't override
  docsy's `scripts.html` (a plugin dispatcher since 0.18) — add scripts through
  the `hooks/body-end.html` hook.
- **Rust deps**: `cargo audit` + `cargo deny` gate CI; keep `deny.toml`'s license
  allow-list tight (a new license fails CI on purpose — review before allowing).

## Documentation

- Public API should have doc comments
- Complex logic deserves inline comments
- Update DEVELOPER_GUIDE.md for architectural changes
- Update README.md for user-facing changes
- **Update docs site**: When making user-facing changes (features, commands, operations, configuration), update the documentation site in `docs/` as part of the same work

## Performance

- Profile before optimizing
- Watch API efficiency is critical - use namespaced APIs when possible
- Defer expensive operations (don't block UI)
- Cache layout dimensions to prevent flicker (see `cached_header_height`)

## Common Gotchas

- Don't forget to invalidate layout cache when state affecting layout changes
- Always check `config.read_only` before write operations
- **YAML goes through `crate::yaml`** (`src/yaml.rs`, serde-saphyr): `yaml::from_str` /
  `yaml::to_string`, with `serde_json::Value` (`preserve_order`) as the dynamic type. It pins
  the parse rules (YAML 1.2 booleans, kubectl-style octal) in one place — never call
  serde-saphyr directly or add `serde_yaml` (deprecated, unmaintained parser; #296)
- **Never edit Secrets**: `objects::fetch_object` redacts them, so applying an edited copy
  would overwrite real values. `KindSpec::with_patch_verb` denies `caps.edit` for Secrets and
  `apply_object_yaml` refuses them as a second guard
- Update both `NO_PROXY` and `no_proxy` for environment compatibility
- Generated models are version-controlled for reproducible builds
- **Kind registry**: `:<kind>` resolves through `models::kinds::resolve` (Flux → Flux-adjacent → discovered). Keep `KindSpec` columns/health/capabilities as data (no fn pointers) so config overlays (#277) stay possible; `nativeResources` is an opt-in preview (default false; `:native` toggles per session) — while off, discovery and kind watches must not run at all, including Flux-adjacent CRD discovery (`Config::flux_crd_discovery_enabled`). See DEVELOPER_GUIDE "The Kind Registry".
- **Never hardcode Flux resource types, API groups, versions, or plural names** - always use `FluxResourceKind` enum and `kube::api::api_resource_for_kind()` helper function to ensure single source of truth
- **Graph node dimensions have one source of truth**: `GraphNode::render_width`/`render_height` (`src/trace/graph.rs`). Both layout (`calculate_layout`) and drawing (`src/tui/views/graph.rs`) call them - never recompute node size inline. Connector geometry is produced by the pure, `Frame`-free `fanout_routes()` so it can be unit tested, separate from the drawing pass.
