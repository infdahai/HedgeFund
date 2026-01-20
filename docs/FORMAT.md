# Opinion HFT - Formatting Standards

**Version**: 1.1.0
**Effective Date**: 2026-01-20
**Status**: Active

---

## Goal

Establish a single, definitive formatting standard to eliminate confusion between rustfmt and clippy rules.

---

## Core Principles

1. **Single Source of Truth**: Only one formatting standard in the project
2. **Automation**: Use rustfmt for automatic formatting, don't rely on manual memory
3. **Enforceable**: Rules can be checked and enforced via tools
4. **Minimalism**: Rules are simple and clear, reducing cognitive burden

---

## 1. rustfmt Configuration (Mechanical Formatting)

### Config File: `rustfmt.toml`

```toml
edition = "2021"
hard_tabs = false
tab_spaces = 4
max_width = 100
```

### Coverage

- ✅ **Indentation**: 4 spaces
- ✅ **Line width**: 100 characters (default)
- ⚠️ **Not covered**: Chained calls, string literals, etc. (keep defaults)

### Rationale

- Rust official standard
- Maximum readability
- Consistent with existing codebase style

---

## 2. clippy Configuration (Linting and Style)

### Project-Specific Rules

Unlike rustfmt defaults, the following rules must be followed:

#### 2.1. Error Handling

- ✅ **Prefer structured error types**: `crate::error::Error`
- ✅ **Use `.into()` to convert error messages**: `.into()` instead of `format!()` (preserves type information)
- ❌ **Forbidden**: String-formatted errors (loses type information)

**Examples**:
```rust
// ✅ Correct
return Err(Error::ApiError {
    status: status.as_u16(),
    message: format!("HTTP error {}", status),
});

// ❌ Wrong
return Err(Error::ApiError(
    format!("HTTP error {}", status).into()
));
```

#### 2.2. Naming Conventions

- ✅ **Types**: `PascalCase` (uppercase first letter)
- ✅ **Functions/Methods**: `snake_case` (underscore-separated)
- ✅ **Constants**: `SCREAMING_SNAKE_CASE` (all caps with underscores)

**Exception**: Naming from external dependencies follows their style

#### 2.3. Structs

- ✅ **Field ordering**: Public API fields first, private fields last
- ✅ **derive macros**: Use `#[derive(Debug, Clone)]` (small types can add `#[derive(Copy)]`)

**Exception**: Test helper structs can be arranged by logical order

#### 2.4. Comments

- ✅ **Public APIs**: Use `///` triple-slash documentation comments
- ⚠️ **Internal implementation**: Prefer `//` double-slash, keep concise
- ❌ **Forbidden**: Block comments `/* */` (unless in temporary code)

**Comment Content Guidelines**:
- Explain "what" (what it is)
- Avoid "how to implement"

**Example**:
```rust
/// Represents a detected arbitrage opportunity.
///
/// This struct contains all information needed to execute
/// an arbitrage trade, including cost estimates and profit margins.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArbitrageOpportunity {
    /// Unique identifier for tracking this opportunity
    pub id: String,

    /// Type of arbitrage strategy detected
    pub arb_type: ArbitrageType,

    // Internal implementation details
    // Timestamp when opportunity was detected
    pub detected_at: u64,
}
```

#### 2.5. Async/Await

- ✅ **spawn and await**: Use `.await` (preserve JoinHandle or cancellation mechanism)
- ✅ **select!**: Multi-way event selection
- ❌ **Forbidden**: Blocking calls (`std::thread::sleep`)

**Examples**:
```rust
// ✅ Correct
let handle = tokio::spawn(async move { /* ... */ });

// ✅ Correct
tokio::select! {
    Some(event) = rx.recv() => { /* ... */ },
    _ = tick.tick() => { /* ... */ },
}
```

#### 2.6. Test Code

- ✅ **Test functions**: Use `#[tokio::test]`
- ✅ **Test naming**: `test_<scenario>_<behavior>` (e.g., `test_fetch_active_markets`)
- ⚠️ **Integration tests**: Require real `.env` configuration, prefixed with `live_`

**Test Data Constants**:
- Minimum order amount: `1.30` USDT
- Minimum fee: `0.50` USDT
- Reasonable price range: `0.3 - 0.7`

---

## 3. Handling Differences from rustfmt/clippy

### 3.1. rustfmt Override: Acceptable Differences

Deviations from rustfmt default format are allowed in these cases:

- **Long URL/path strings**: May exceed 100 character limit
- **Complex error messages**: Long lines caused by `format!()` macros

### 3.2. clippy Warning Handling Strategy

- **Fix immediately**: Warnings that block compilation must be fixed first
- **Technical debt**: Non-blocking warnings can be recorded as technical debt, allow follow-up handling
- **False positives**: If confirmed as false positives, add `#[allow(...)]`

---

## 4. Execution Workflow

### Development Phase

1. **Code Changes**
    ```bash
    # After editing code
    cargo fmt --all
    ```

2. **Checks**
    ```bash
    # Check formatting (optional)
    cargo fmt --all --check

    # Check lint warnings (must have 0 warnings)
    cargo clippy --all -D warnings
    ```

3. **Pre-commit Checklist**
    - [ ] `cargo fmt` passes (no formatting changes)
    - [ ] `cargo clippy` has no warnings (0 warnings)
    - [ ] All public APIs have documentation comments
    - [ ] No unsafe code (unless documented)

### CI/CD Integration

**GitHub Actions / Automation Tools**:
- Add pre-commit hook to automatically run `cargo fmt`
- Run `cargo clippy --all -D warnings` in CI and block non-zero exit codes

---

## 5. FAQ (Frequently Asked Questions)

### Q1: Why do rustfmt and clippy rules conflict?

**A**: They focus on different aspects:
- **rustfmt**: Mechanical formatting (indentation, spacing, line breaks)
- **clippy**: Code quality and best practices (correctness, performance, complexity)

**Conclusion**: They complement each other, no conflicts.

### Q2: How to handle legacy code that doesn't conform to rules?

**A**:
1. Run `cargo fmt --all` to batch format
2. Commit formatting changes separately
3. Gradually fix clippy warnings afterward

### Q3: Can test code not follow the rules?

**A**: Test helper structs can simplify rules, but core test logic must follow naming and error handling conventions.

---

## Changelog

| Version | Date | Changes |
|---------|------|---------|
| 1.0 | 2026-01-20 | Initial version, established unified formatting standard |

---

## References

- [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/)
- [Rust Style Guide](https://rust-lang.github.io/style-guide/)
- [clippy Lints Documentation](https://rust-lang.github.io/rust-clippy/)
