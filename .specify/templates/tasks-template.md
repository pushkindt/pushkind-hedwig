# Tasks: [FEATURE NAME]

**Input**: Design documents from `/specs/[###-feature-name]/`
**Prerequisites**: plan.md (required), research.md, data-model.md, contracts/

## Execution Flow (main)
```
1. Load plan.md from feature directory
   → If not found: ERROR "No implementation plan found"
   → Extract: tech stack, libraries, structure
2. Load optional design documents:
   → data-model.md: Extract entities → model tasks
   → contracts/: Each file → contract test task
   → research.md: Extract decisions → setup tasks
3. Generate tasks by category:
   → Setup: project init, dependencies, migrations, configuration
   → Tests: contract tests, integration tests, failure simulations
   → Core: models, services, async workers, CLI commands
   → Reliability: retry/backoff logic, idempotency checks, replay handling
   → Security: secret management, TLS validation, audit logging
   → Observability: structured logging, metrics, runbook updates
   → Polish: unit tests, docs, performance validation
4. Apply task rules:
   → Different files = mark [P] for parallel
   → Same file = sequential (no [P])
   → Tests before implementation (TDD)
5. Number tasks sequentially (T001, T002...)
6. Generate dependency graph
7. Create parallel execution examples
8. Validate task completeness:
   → All contracts have tests?
   → All entities have models?
   → Reliability, security, and observability work items covered?
9. Return: SUCCESS (tasks ready for execution)
```

## Format: `[ID] [P?] Description`
- **[P]**: Can run in parallel (different files, no dependencies)
- Include exact file paths in descriptions

## Path Conventions
- Single project: `src/`, `tests/`, `migrations/` at repository root
- Rust integration tests live in `tests/` (crate style) or `src/bin/` for binaries
- Configuration lives in environment variables or `config.toml`

## Phase 3.1: Setup
- [ ] T001 Establish feature module skeleton in `src/` (create mod file, wire into `lib.rs`)
- [ ] T002 Update configuration validation in `src/bin/*.rs` or `config` module to include new settings
- [ ] T003 [P] Prepare Diesel migration/seed data in `migrations/` if schema changes are required

## Phase 3.2: Tests First (TDD) ⚠️ MUST COMPLETE BEFORE 3.3
**CRITICAL: These tests MUST be written and MUST FAIL before ANY implementation**
- [ ] T004 [P] Contract test for new message schema in `tests/contract/[feature]_contract.rs`
- [ ] T005 [P] Integration test happy path in `tests/integration/[feature]_happy_path.rs`
- [ ] T006 [P] Integration test retry/backoff behavior in `tests/integration/[feature]_retries.rs`
- [ ] T007 [P] Security regression test (credential handling/TLS failure) in `tests/integration/[feature]_security.rs`

## Phase 3.3: Core Implementation (ONLY after tests are failing)
- [ ] T008 Implement domain models/repository updates in `src/repository/`
- [ ] T009 Implement service logic in `src/send_email/` or `src/check_reply/` as applicable
- [ ] T010 Extend async worker or command entry point in `src/bin/[binary].rs`
- [ ] T011 Add retry/backoff and idempotency guards in `src/[module]/`
- [ ] T012 Handle secrets via injected configuration and typed errors (no `unwrap`/`expect`)

## Phase 3.4: Integration & Observability
- [ ] T013 Wire structured logging with hub/message identifiers in `src/[module]/`
- [ ] T014 Emit metrics/telemetry counters for queued/sent/retried/failed messages
- [ ] T015 Update runbook or ops documentation in `docs/` or `/specs/.../plan.md`
- [ ] T016 Verify ZMQ topic subscriptions/publishing rules and document changes

## Phase 3.5: Security & Polish
- [ ] T017 [P] Add unit tests for error handling and edge cases in `tests/unit/[feature]_errors.rs`
- [ ] T018 Audit credentials and configuration usage; ensure secrets stay external
- [ ] T019 [P] Run `cargo fmt`, `cargo clippy --all-features --tests -- -D warnings`, and `cargo test --all-features --verbose`
- [ ] T020 Document public API changes with Rustdoc and update `CHANGELOG` or release notes if required

## Dependencies
- Tests (T004-T007) before implementation (T008-T012)
- Retry/observability work (T011-T014) depends on core implementation
- Configuration updates (T002) block service wiring (T010)
- Security polish (T018) depends on implementation and observability completion

## Parallel Example
```
# Launch T004-T007 together once design is approved:
Task: "Contract test for new message schema in tests/contract/[feature]_contract.rs"
Task: "Integration test happy path in tests/integration/[feature]_happy_path.rs"
Task: "Integration test retry/backoff behavior in tests/integration/[feature]_retries.rs"
Task: "Security regression test in tests/integration/[feature]_security.rs"
```

## Notes
- [P] tasks = different files, no dependencies
- Verify tests fail before implementing features
- Record reliability/security/observability decisions in plan.md and tasks.md
- Avoid: vague tasks, same file conflicts, merging without passing CI

## Task Generation Rules
*Applied during main() execution*

1. **From Contracts**:
   - Each contract file → contract test task [P]
   - Each endpoint/event → implementation task
2. **From Data Model**:
   - Each entity → model creation task [P]
   - Relationships → repository/service updates
3. **From User Stories**:
   - Each story → integration test [P]
   - Quickstart scenarios → validation tasks
4. **From Constitution Requirements**:
   - Add tasks for reliability (retries, idempotency), security (secrets, TLS), and observability (logs, metrics) explicitly.

5. **Ordering**:
   - Setup → Tests → Models/Services → Reliability/Security/Observability → Polish
   - Dependencies block parallel execution

## Validation Checklist
*GATE: Checked by main() before returning*

- [ ] All contracts have corresponding tests
- [ ] All entities have model tasks
- [ ] Tests precede implementation
- [ ] Reliability, security, and observability tasks present
- [ ] Parallel tasks truly independent
- [ ] Each task specifies exact file path
- [ ] No task modifies same file as another [P] task
- [ ] Final tasks cover CI commands and documentation updates
