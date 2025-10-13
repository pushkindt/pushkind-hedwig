# AGENTS.md

Guidance for AI-assisted changes in this repository. Follow these notes so new
code fits the existing layout and operational expectations.

## Project Context

- `pushkind-hedwig` ships two Tokio-based background workers: `send_email`
  consumes ZeroMQ messages and delivers SMTP mail, while `check_reply` polls
  IMAP, persists status updates, and publishes follow-up events.
- Domain data, Diesel models, schema definitions, and shared ZeroMQ payload
  types live in `pushkind-common`. This crate focuses on orchestration and
  worker-specific glue.
- Persistence relies on Diesel + SQLite with a connection pool provided by
  `pushkind_common::db`. Business logic is expressed in service modules;
  repository code stays focused on I/O.

## Code Layout

- `src/bin/*.rs` provide small entry points that initialise logging, load
  environment variables, and run the workers.
- `src/send_email` contains the outbound email pipeline:
  - `message_builder.rs` builds MIME messages; keep it pure and side-effect
    free.
  - `service.rs` coordinates repositories, hubs, and the `Mailer` trait. Place
    business rules here, not in the bin targets.
- `src/check_reply` handles inbound mail:
  - `imap.rs` owns IMAP connectivity.
  - `parser.rs` extracts reply content and unsubscribe semantics.
  - `service.rs` runs the monitoring loop and talks to ZeroMQ.
- `src/repository` defines the `DieselRepository` plus the `Email*` and
  `Hub*` traits that services depend on. Extend traits before reaching into the
  concrete type directly.
- `src/models.rs` hosts Diesel-only structs (currently the unsubscribe insert
  helper). Keep Diesel-specific representations here.
- `tests/` holds integration tests that exercise the Diesel layer and services
  using temporary SQLite databases; reuse `tests/common::TestDb`.

## External Dependencies & Patterns

- Async workloads run on Tokio. Avoid blocking calls inside async contexts; use
  `tokio::task::spawn_blocking` if CPU-bound work is unavoidable.
- ZeroMQ communication uses `zmq` directly in the sender worker and
  `pushkind_common::zmq::ZmqSender` for publishing replies/unsubscribes. Prefer
  the helpers from `pushkind-common` instead of raw sockets whenever possible.
- SMTP delivery is encapsulated behind the `Mailer` trait; add alternative
  mailers by implementing that trait.
- Centralise failure handling through `crate::errors::Error`. Add new variants
  with `thiserror` and bubble them up instead of panicking.

## Development Commands

Run the same checks locally that CI expects:

```bash
cargo fmt --all
cargo clippy --all-features --all-targets --tests -- -Dwarnings
cargo test --all-features
```

Use `cargo build --all-targets --all-features` when you need a full compile
check outside the test flow.

## Coding Standards

- Prefer idiomatic Rust; avoid `unwrap`/`expect` in shipped code paths. Convert
  configuration issues into `Error::Config` and propagate repository failures as
  `RepositoryError`.
- Keep services generic over repository traits (`EmailReader + EmailWriter +
  HubReader`, etc.) so Diesel and test doubles remain interchangeable.
- Repositories should handle Diesel conversions and return domain types from
  `pushkind_common::domain`. Any Diesel-specific structs should implement
  `From`/`Into` conversions in place.
- When spawning background tasks (e.g., in the email sender loop), clone the
  repository and other handles explicitly and surface errors via logging rather
  than panics.
- Use structured logging with the `log` crate to aid observability, following
  the existing style (`log::info!`, `log::error!`, etc.).
- Document new public APIs or behavioural changes with Rustdoc comments.

## Database Guidelines

- Use Diesel’s query builder with `pushkind_common::schema::emailer::*`; avoid
  raw SQL in production code. Wrap multi-step mutations in `conn.transaction`
  blocks.
- Keep repository methods small: establish a pooled connection via
  `self.conn()`, perform the query, and convert results into domain structs.
- When persisting related data (emails plus recipients, IMAP UIDs, unsubscribes)
  update aggregate counters via the existing helpers (e.g., `DbEmail::recalc_*`)
  rather than manual SQL.
- Return `RepositoryResult<T>` and translate missing rows into
  `RepositoryError::NotFound` instead of panicking.

## ZeroMQ, IMAP, and SMTP Practices

- When consuming ZeroMQ messages, validate payloads with Serde and log parse
  errors before continuing the loop—never terminate the worker on bad input.
- Keep IMAP sessions resilient: retry transient failures with backoff like the
  existing monitor loop, persist advancing UIDs through `HubWriter`, and guard
  against integer overflow when converting UIDs.
- Treat unsubscribe emails specially by calling `unsubscribe_recipient` and
  publishing a `ZMQUnsubscribeMessage`; follow the established pattern in
  `check_reply::service`.
- Ensure outbound email updates recipient status via `update_recipient`
  immediately after a successful send so metrics stay accurate.

## Testing Expectations

- Add unit tests for pure logic in `message_builder`, `parser`, and similar
  modules. Use `#[tokio::test]` for async routines.
- For repository and service tests that need a database, follow the existing
  temp-DB pattern (`tempfile::TempDir` + `TestDb::new`) and create the schema
  upfront.
- Mock dependencies through traits (e.g., fake `Mailer` implementations) to
  keep tests deterministic. Prefer exercising real Diesel code paths in
  integration tests.
- Ensure new functionality is covered by tests before finishing a change; the
  default `cargo test --all-features` should pass cleanly.

Adhering to these conventions keeps the workers reliable and consistent with the
rest of the Pushkind stack.
