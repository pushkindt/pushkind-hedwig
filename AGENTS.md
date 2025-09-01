# AGENTS.md

This file provides guidance to AI code generators when working with the code in this repository.

## Development Commands

Use these commands to verify your changes before committing:

**Build**
```bash
cargo build --all-features --verbose
```

**Run Tests**
```bash
cargo test --all-features --verbose
```

**Lint (Clippy)**
```bash
cargo clippy --all-features --tests -- -Dwarnings
```

**Format**
```bash
cargo fmt --all -- --check
```

### Key Development Rules

- Use idiomatic Rust everywhere
- Follow the Clean Code and Clean Architecture principles
- Use `thiserror` for error definitions; avoid `anyhow::Result`
- Define error types inside their unit of fallibility
- Document all public APIs and breaking changes
- Always run formatting and linting before create PRs

