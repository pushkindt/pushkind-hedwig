# pushkind-hedwig

An email sender and reply receiver for Pushkind services
`pushkind-hedwig` hosts the Tokio background workers that power outbound and inbound
email for Pushkind hubs. The `send_email` worker consumes ZeroMQ delivery requests,
persists messages, and delivers them over SMTP, while `check_reply` polls IMAP inboxes
to record replies, manage unsubscribes, and broadcast follow-up events. Both binaries
lean on `pushkind-common` for domain models, database access, and shared ZeroMQ payloads.

## Features

- **ZeroMQ-driven dispatch** – `send_email` subscribes to `ZMQSendEmailMessage` payloads,
  persists new emails when needed, and drives SMTP delivery for each recipient.
- **Template-aware message builder** – `send_email::message_builder` merges hub templates,
  recipient fields, tracking pixels, unsubscribe links, and optional attachments.
- **Resilient IMAP monitoring** – `check_reply` maintains per-hub loops that resume from the
  last processed UID, retries on transient failures, and keeps the inbox session alive.
- **Reply and unsubscribe propagation** – Incoming mail is parsed for replies, bounces,
  and unsubscribe requests; recipient state is updated and ZeroMQ messages are published.
- **SQLite-backed repository** – `DieselRepository` wraps pooled SQLite access to read hubs,
  store emails, persist IMAP cursors, and record unsubscribe actions.
- **Structured observability** – Workers initialise `env_logger` and emit contextual logs so
  operations can trace message flow without dropping the loop.

## Architecture at a Glance

The repository is organised around two long-running workers and the shared
infrastructure they depend on:

- **Worker entrypoints (`src/bin`)** – Small binaries that configure logging,
  load environment variables, and call into the worker modules.
- **Outbound pipeline (`src/send_email`)** – Owns the SMTP mailer, message
  builder, and orchestration logic for processing `ZMQSendEmailMessage` payloads.
- **Inbound pipeline (`src/check_reply`)** – Manages IMAP connectivity, parses
  inbound messages, and publishes reply/unsubscribe events via ZeroMQ.
- **Repository (`src/repository`)** – Traits plus the `DieselRepository` that
  wraps SQLite access for hubs, emails, recipients, and IMAP state.
- **Models (`src/models.rs`)** – Diesel-specific structs that complement the
  shared schema (e.g. unsubscribe helpers).
- **Errors (`src/errors.rs`)** – Central error type that consolidates failure
  modes surfaced by the workers.
- **Domain (`pushkind-common::domain`)** – Domain models, schema definitions, and
  ZeroMQ payload types come from the shared `pushkind-common` crate and are used
  directly by the workers.
- **Tests (`tests/`)** – Integration tests that exercise the repository and
  service flows with temporary SQLite databases.

Because the repository traits live in `src/repository/mod.rs`, worker logic
remains generic over those traits so database-backed and test doubles can be
swapped in easily.

## Technology Stack

- Rust 2024 edition
- [Actix Web](https://actix.rs/) with identity, session, and flash message
  middleware
- [Diesel](https://diesel.rs/) ORM with SQLite and connection pooling via r2d2
- [Tera](https://tera.netlify.app/) templates styled with Bootstrap 5.3
- [`pushkind-common`](https://github.com/pushkindt/pushkind-common) shared crate
  for authentication guards, configuration, database helpers, and reusable
  patterns
- Supporting crates: `chrono`, `validator`, `serde`, `ammonia`, `csv`, and
  `thiserror`

## Getting Started

### Prerequisites

- Rust toolchain (install via [rustup](https://www.rust-lang.org/tools/install))
- `diesel-cli` with SQLite support (`cargo install diesel_cli --no-default-features --features sqlite`)
- SQLite 3 installed on your system

### Environment

The service reads configuration from environment variables. The most important
ones are:

| Variable | Description | Default |
| --- | --- | --- |
| `DATABASE_URL` | SQLite connection string used by both workers | `app.db` |
| `DOMAIN` | Domain suffix used for tracking pixels and message IDs | _(empty)_ |
| `ZMQ_EMAILER_SUB` | ZeroMQ endpoint the sender subscribes to for delivery jobs | `tcp://127.0.0.1:5558` |
| `ZMQ_REPLIER_PUB` | ZeroMQ endpoint the reply worker publishes events to | `tcp://127.0.0.1:5559` |

Add these to a `.env` file to have them loaded automatically via
[`dotenvy`](https://crates.io/crates/dotenvy) when either worker starts.

### Database

Run the Diesel migrations before starting the server:

```bash
diesel setup
cargo install diesel_cli --no-default-features --features sqlite # only once
diesel migration run
```

A SQLite file will be created at the location given by `DATABASE_URL`.

## Running the Application

Start the HTTP server with:

```bash
cargo run
```

The server listens on `http://127.0.0.1:8080` by default and serves static
assets from `./assets` in addition to the Tera-powered HTML pages. Authentication
and authorization are enforced via the Pushkind auth service and the
`SERVICE_ACCESS_ROLE` constant.

## Quality Gates

The project treats formatting, linting, and tests as required gates before
opening a pull request. Use the following commands locally:

```bash
cargo fmt --all -- --check
cargo clippy --all-features --tests -- -Dwarnings
cargo test --all-features --verbose
cargo build --all-features --verbose
```

Alternatively, the `make check` target will format the codebase, run clippy, and
execute the test suite in one step.

## Testing

Unit tests exercise the service and form layers directly, while integration
tests live under `tests/`. Repository tests rely on Diesel’s query builders and
should avoid raw SQL strings whenever possible. Use the mock repository module to
isolate services from the database when writing new tests.

## Project Principles

- **Domain-driven**: keep business rules in the domain and service layers and
  translate to/from external representations at the boundaries.
- **Explicit errors**: use `thiserror` to define granular error types and convert
  them into `ServiceError`/`RepositoryError` variants instead of relying on
  `anyhow`.
- **No panics in production paths**: avoid `unwrap`/`expect` in request handlers,
  services, and repositories—propagate errors instead.
- **Security aware**: sanitize any user-supplied HTML using `ammonia`, validate
  inputs with `validator`, and always enforce role checks with
  `pushkind_common::routes::check_role`.
- **Testable**: accept traits rather than concrete types in services and prefer
  dependency injection so the mock repositories can be used in tests.

Following these guidelines will help new functionality slot seamlessly into the
existing architecture and keep the service reliable in production.
