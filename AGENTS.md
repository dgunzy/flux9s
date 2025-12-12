# flux9s Development Instructions

## Core Philosophy

- **Zero-maintenance model updates**: Use code generation over manual types
- **Non-blocking operations**: Keep UI responsive with async task spawning
- **Graceful degradation**: Log errors, don't crash (only panic for truly unrecoverable errors)
- **K9s alignment**: Follow K9s conventions for keybindings and UX

## Before You Start

1. Run `make ci` after all changes (formats, lints, tests)
2. Large changes require accompanying tests
3. Command/keybinding changes must update help text (`src/tui/views/help.rs` and footer)

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
3. Add to `FluxResourceKind` enum
4. Add `impl_watchable!` macro usage
5. Register in `ResourceRegistry`
6. Add to `fetch_resource!` match in `src/tui/mod.rs`
7. Update tests in `tests/resource_registry.rs`

**Important**: Always use `FluxResourceKind` enum and `get_gvk_for_resource_type()` helper function
instead of hardcoding resource types, API groups, versions, or plural names. This ensures a single
source of truth and prevents inconsistencies when Flux versions change.

### Adding New Operations

1. Implement `FluxOperation` trait
2. Register in `OperationRegistry::new()`
3. Add keybinding to footer (`src/tui/views/footer.rs`)
4. Add to help text (`src/tui/views/help.rs`)
5. Write operation tests

### View Components

- Keep views stateless - they receive all needed data as parameters
- Extract complex rendering logic into helper functions
- Use theme colors consistently (never hardcode colors)
- Handle small terminal sizes gracefully

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

## CI/CD

- `make ci` runs: `cargo fmt`, `cargo clippy`, `cargo test`
- All CI checks must pass before merge
- Clippy warnings are treated as errors
- Generated code in `src/models/_generated/` has clippy suppressed

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
- Update both `NO_PROXY` and `no_proxy` for environment compatibility
- Generated models are version-controlled for reproducible builds
- **Never hardcode Flux resource types, API groups, versions, or plural names** - always use `FluxResourceKind` enum and `get_gvk_for_resource_type()` helper function to ensure single source of truth
