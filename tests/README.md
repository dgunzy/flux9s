# Testing Guide

This directory contains tests for the flux9s project.

## Test Organization

```
tests/
├── unit/              # Unit tests for individual components
│   ├── models/        # Model layer tests
│   ├── watcher/       # Watcher layer tests
│   ├── tui/           # TUI layer tests
│   └── operations/    # Operations tests
├── integration/       # Integration tests
│   ├── api/           # Kubernetes API integration tests
│   ├── watcher/       # End-to-end watcher tests
│   └── tui/           # End-to-end TUI tests
└── fixtures/          # Test data and fixtures
    ├── crds/          # Sample CRD definitions
    └── resources/     # Sample resource JSON responses
```

## Running Tests

```bash
# Run all tests
cargo test

# Run unit tests only
cargo test --test unit

# Run integration tests only
cargo test --test integration

# Run tests with output
cargo test -- --nocapture

# Run specific test
cargo test test_name
```

## Test Infrastructure

### Mocking

We use `mockall` for trait mocking in unit tests. See `Cargo.toml` for dependencies.

### Test Fixtures

Test fixtures are stored in `tests/fixtures/` and include:
- Sample CRD definitions
- Sample Kubernetes resource JSON responses
- Mock watch events

### Kubernetes API Testing

For integration tests that require Kubernetes API access:
- Use `kube::Client` with test fixtures
- Test against kind/minikube clusters
- Mock API responses where possible

## Writing Tests

### Unit Test Example

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resource_key_generation() {
        let key = resource_key("default", "my-resource", "Kustomization");
        assert_eq!(key, "default/my-resource/Kustomization");
    }
}
```

### Integration Test Example

```rust
#[tokio::test]
async fn test_watch_stream() {
    // Setup mock Kubernetes client
    // Test watch stream handling
    // Verify state updates
}
```

## Test Coverage Goals

- **Models:** 80%+ coverage
- **Watcher:** 70%+ coverage
- **TUI:** 60%+ coverage (UI is harder to test)
- **Operations:** 80%+ coverage

## CI/CD Testing

Tests run automatically on:
- Every PR (unit tests)
- Every merge (unit + integration tests)
- Nightly builds (full test suite)

