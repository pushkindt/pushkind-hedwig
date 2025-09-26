<!--
Sync Impact Report
Version change: N/A -> 1.0.0
Modified principles:
- placeholder PRINCIPLE_1_NAME -> End-to-End Mail Reliability
- placeholder PRINCIPLE_2_NAME -> Secure Identity & Compliance
- placeholder PRINCIPLE_3_NAME -> Observable Operations
- placeholder PRINCIPLE_4_NAME -> Rust Discipline
- placeholder PRINCIPLE_5_NAME -> Automated Change Control
Added sections: Core Principles, Operational Guardrails, Development Workflow, Governance
Removed sections: None
Templates requiring updates:
- ✅ .specify/templates/plan-template.md
- ✅ .specify/templates/spec-template.md
- ✅ .specify/templates/tasks-template.md
Follow-up TODOs: None
-->
# Pushkind Hedwig Constitution

## Core Principles

### End-to-End Mail Reliability
- Outbound senders and inbound monitors MUST guarantee at-least-once delivery with idempotent processing so retries never create duplicates or drop replies.
- Every feature MUST define failure handling for SMTP, IMAP, ZMQ, and database dependencies, including retry intervals and safe backoff.
- Automated tests MUST cover success, transient failure, and recovery paths before implementation ships.
**Rationale:** Our value depends on matching every outbound message with its reply; codifying durable, test-backed flows prevents data loss when providers misbehave.

### Secure Identity & Compliance
- Secrets, credentials, and PII MUST stay in managed secret storage; no hard-coded or logged secrets.
- All network transport MUST terminate with verified TLS via `rustls` or documented equivalent, failing closed when trust cannot be established.
- Data access MUST follow least privilege by using repository APIs that enforce tenant-level scoping and audit logging.
**Rationale:** We transport sensitive customer communications; strict credential hygiene and scoped access reduce breach surface and support compliance audits.

### Observable Operations
- Long-running workers MUST emit structured logs (domain, hub, message identifiers) for each lifecycle event and failure.
- Features MUST expose counters or traces for queued, sent, retried, and failed messages in a format compatible with Pushkind monitoring.
- Every incident path MUST document expected operator actions and success signals inside the plan or runbook artifacts.
**Rationale:** Without traceable telemetry the on-call team cannot distinguish platform noise from message loss; observability keeps SLAs defensible.

### Rust Discipline
- Production code MUST avoid `unwrap`/`expect`; use typed errors via `thiserror` and propagate failures explicitly.
- Tests MUST precede implementation (TDD): add failing integration/contract tests before modifying mailer or parser code.
- Public APIs, binaries, and feature flags MUST include Rustdoc explaining purpose and invariants.
**Rationale:** Rigorous Rust patterns prevent panics, make failures visible to callers, and keep contributors aligned with Pushkind engineering standards.

### Automated Change Control
- `cargo fmt --all`, `cargo clippy --all-features --all-targets --tests`, and `cargo test --all-targets --all-features` MUST pass in CI and locally before merge.
- Feature branches MUST document migration impact and rollback plans inside `/specs/.../plan.md` before implementation starts.
- Deployments MUST be reproducible from tagged releases; configuration drift requires a tracked remediation task.
**Rationale:** Automated gates and explicit rollback data reduce regressions in a service that runs continuously on live customer mail streams.

## Operational Guardrails

- **Runtime configuration:** Workers MUST load configuration from environment variables or `config.toml`, validate presence once at startup, and crash fast on missing values.
- **Asynchronous safety:** All new blocking IO MUST be wrapped in Tokio-compatible async operations; introduce dedicated threads only with documented justification in the plan.
- **Database usage:** Use the `DieselRepository` and shared `pushkind-common` models for all hub queries; raw SQL requires a compliance review.
- **Messaging fabric:** ZMQ subjects MUST be namespaced per environment and authenticated senders; adding new topics requires updating monitoring checklists.
- **Security hygiene:** Rotate SMTP/IMAP credentials through secret management tooling; never persist credentials in source, tests, or logs.

## Development Workflow

1. Capture user needs in `/specs` using the spec template; resolve all `[NEEDS CLARIFICATION]` items before planning.
2. Generate `/specs/.../plan.md` and perform the Constitution Check twice (pre- and post-design) to prove compliance with the five principles.
3. Produce `/specs/.../tasks.md` only after design artifacts are complete; tasks MUST schedule tests before implementation and document rollback hooks.
4. Implement via short-lived branches, following TDD and pairing with reviewers to validate observability and security requirements.
5. Before merge, run `cargo fmt --all`, `cargo clippy --all-features --all-targets --tests -- -D warnings`, and `cargo test --all-features --all-targets --verbose`; attach output or rerun until clean.
6. Update runtime documentation or runbooks whenever operational guardrails change; missing updates block release.

## Governance

- Amendments require an RFC issue describing the motivation, principle impact, and rollout plan; approval by the service owner and one peer maintainer is mandatory.
- Once approved, update this constitution, templates, and any affected runbooks within the same pull request; semantic version bumps follow MAJOR/MINOR/PATCH rules based on impact.
- Record the new version, ratification, and amendment dates in this document and reference them in release notes.
- Perform a quarterly compliance review covering code, tests, and observability dashboards; unresolved findings must become tracked tasks.
- Non-compliance discovered during reviews or incidents MUST trigger remediation within the next sprint or a documented risk acceptance.

**Version**: 1.0.0 | **Ratified**: 2025-09-26 | **Last Amended**: 2025-09-26
