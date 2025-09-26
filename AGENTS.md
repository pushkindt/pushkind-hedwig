# AGENTS.md

This file provides guidance to AI code generators when working with the code in
this repository.

## Development Commands

Use these commands to verify your changes before committing:

**Build**
```bash
cargo build --all-features --all-targets --verbose
```

**Run Tests**
```bash
cargo test --all-features --all-targets  --verbose
```

**Lint (Clippy)**
```bash
cargo clippy --all-features --all-targets --tests -- -Dwarnings
```

**Format**
```bash
cargo fmt --all -- --check
```

### Key Development Rules

- Use idiomatic Rust everywhere, avoid .unwrap(), .expect(), and clone() where
possible
- Follow the Clean Code, Clean Architecture, DDD, and TDD principles
- Depend on traits, not on concrete impementations
- Start from Domain-level data structures and build code around them
- Use layered approach: domain, service, repository.
- Use `thiserror` for error definitions; avoid `anyhow::Result`
- Define error types inside their unit of fallibility
- Document all public APIs and breaking changes
- Always run formatting and linting before create PRs
