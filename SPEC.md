# pushkind-hedwig — Specification

`pushkind-hedwig` is a pair of Tokio-based background workers that implement outbound email delivery (SMTP) and inbound reply/unsubscribe detection (IMAP), coordinated via ZeroMQ and persisted via Diesel + SQLite.

Relevant code:

- Outbound worker: `src/send_email/mod.rs`, `src/send_email/service.rs`, `src/send_email/message_builder.rs`
- Inbound worker: `src/check_reply/mod.rs`, `src/check_reply/service.rs`, `src/check_reply/imap.rs`, `src/check_reply/parser.rs`
- Persistence traits + Diesel impl: `src/repository/mod.rs`, `src/repository/email.rs`, `src/repository/hub.rs`
- Error model: `src/errors.rs`

## Goals

- Consume delivery jobs from ZeroMQ, persist new emails (when needed), and deliver SMTP messages to each recipient.
- Build messages from hub templates and per-recipient fields, injecting:
  - A stable `Message-ID` that allows reply correlation.
  - A tracking pixel URL.
  - A `List-Unsubscribe` header and unsubscribe link.
  - Optional file attachments when present.
- Monitor IMAP inboxes per hub, resume from the last processed UID, and:
  - Detect replies and persist recipient state updates.
  - Detect unsubscribe requests and bounce notifications and persist unsubscribes.
  - Publish reply/unsubscribe events back to the rest of the system via ZeroMQ.
- Keep long-running loops resilient: log and continue on bad input; retry transient IMAP failures with backoff; avoid panics on operational paths.

## Non-goals

- Providing an HTTP API or web UI (this crate ships only the `send_email` and `check_reply` binaries).
- Guaranteeing “exactly once” delivery semantics across process restarts (delivery is best-effort per recipient; no explicit job ACK protocol exists here).
- Full email-client reply threading support (reply correlation relies on parsing `In-Reply-To` only).
- Implementing deliverability features beyond what’s encoded in templates/headers (DKIM/DMARC signing, bounce classification beyond the current heuristics, etc.).

## Domain Model

Most domain types come from `pushkind-emailer` (and are used directly here). The locally-defined domain helper is:

- `UpdateEmailRecipient` (`src/domain.rs`): partial updates applied to an `EmailRecipient` record:
  - `sent: Option<bool>`
  - `opened: Option<bool>`
  - `reply: Option<&EmailRecipientReply>`

Key entities as used by the workers:

- **Hub**
  - Identity: `HubId`
  - SMTP config: `smtp_server`, `smtp_port`, `login`, `password`, `sender`
  - IMAP config: `imap_server`, `imap_port`, `login`, `password`
  - Template: `email_template` (HTML/text body template)
  - Cursor: `imap_last_uid` (monotonic “last processed UID”)
  - Derived: `hub.unsubscribe_url()` (used for the `List-Unsubscribe` header and `{unsubscribe_url}` placeholder).
- **Email**
  - Identity: `EmailId`
  - Fields: `message`, `subject`, optional attachment triple `(attachment, attachment_name, attachment_mime)`
  - Counters: `num_sent`, `num_opened`, `num_replied` (recalculated by the repository on recipient updates).
  - Foreign key: `hub_id`.
- **EmailRecipient**
  - Identity: `EmailRecipientId`
  - Fields: `address`, `name`, `fields` (map used for templating), `is_sent`, `opened`, `reply`, `updated_at`.
- **Unsubscribe**
  - Stored as `(hub_id, email_address, reason)` and inserted idempotently (conflicts are ignored).

## Invariants

- **Hub scoping for email data**
  - Reads for emails and recipients are always constrained by hub ownership (repository joins recipients ↔ emails and filters by `emails.hub_id`).
- **Recipient-driven reply correlation**
  - Outbound `Message-ID` is `"{recipient_id}@{domain}"` (see `src/send_email/message_builder.rs`).
  - Inbound correlation extracts an integer local-part from `In-Reply-To` values containing `<id@{domain}>` (see `src/check_reply/parser.rs`).
- **Template rendering behavior**
  - Email body uses a two-stage placeholder replacement:
    1. Render `email.message` using `recipient.fields` only.
    2. Render `hub.email_template` (or `{message}` by default) with `{name}`, `{unsubscribe_url}`, and `{message}`.
  - Unknown placeholders are left intact (e.g., `{favourite fruit}` remains `{favourite fruit}`).
  - If `hub.email_template` is missing `{message}`, it is appended as a new paragraph.
- **Tracking pixel**
  - Every outbound message includes an HTML pixel: `https://mail.{domain}/track/{recipient_id}`.
  - The scheme/host/path are currently fixed in code; only `{domain}` is configurable via `ServerConfig.domain`.
  - `domain` must correspond to a publicly reachable HTTP host that serves `/track/{recipient_id}` for tracking to function.
- **Unsubscribe persistence**
  - Unsubscribes are idempotent for the tuple `(hub_id, email)` (`ON CONFLICT DO NOTHING`).
- **IMAP cursor monotonicity**
  - `imap_last_uid` only advances; candidates that do not fit `i32`, do not pass `ImapUid` validation, or are `<=` the stored UID are ignored.
  - UIDs are processed in sorted order per fetch cycle.

## API Contracts

### Configuration

Both binaries load `ServerConfig` (`src/models.rs`) via the `config` crate:

- Base: `config/default.yaml`
- Override: `config/{APP_ENV}.yaml` (optional; `APP_ENV` defaults to `local`)
- Environment: variables with prefix `APP_` (e.g., `APP_DATABASE_URL`)

`ServerConfig` fields and expected meaning:

- `domain`: domain suffix used in outbound `Message-ID` and tracking URLs, and in inbound `In-Reply-To` parsing.
- `database_url`: SQLite path/URL consumed by `pushkind_common::db::establish_connection_pool`.
- `zmq_emailer_sub`: `send_email` subscribes to this address (raw `zmq::SUB`).
- `zmq_replier_pub`: `check_reply` publishes to this address (via `pushkind_common::zmq::ZmqSender`).
- Additional config keys exist in `ServerConfig` but are currently unused by the binaries (`zmq_emailer_pub`, `zmq_replier_sub`).

### Hub discovery lifecycle

- `check_reply` discovers hubs once at startup via `HubReader::list_hubs()` and spawns one monitor task per hub ID.
- While running, it does not discover newly added hubs automatically; adding a hub requires restarting `check_reply` to begin monitoring it.
- If a hub is removed while running, the monitor task for that hub continues retrying and logs `Hub#{id} not found` until the hub reappears.
- Hub configuration updates are picked up on the next restart attempt of the per-hub loop because it re-fetches the hub record via `get_hub_by_id(hub_id)` before reconnecting to IMAP.

### ZeroMQ payloads

The wire formats are JSON (Serde) for types from `pushkind_emailer::models::zmq`:

- `ZMQSendEmailMessage`
  - `RetryEmail((email_id, hub_id))`: fetch existing email data from DB before sending.
  - `NewEmail((user, new_email))`: persist `new_email` and send it (the `user` value is currently ignored by Hedwig).
- `ZMQReplyMessage` (published by `check_reply`)
  - `hub_id: i32`
  - `email: String` (sender email address extracted from headers)
  - `message: String` (reply text; empty when not available)
  - `subject: Option<String>`
- `ZMQUnsubscribeMessage` (published by `check_reply`)
  - `hub_id: i32`
  - `email: String` (email address being unsubscribed/bounced)
  - `reason: Option<String>` (currently the triggering subject)

### ZMQ delivery semantics and ordering

- Consumers must tolerate duplicate `ZMQSendEmailMessage` deliveries (ZeroMQ SUB sockets provide at-most-once delivery per connection, but the system as a whole can still produce duplicates on retry/restart).
- `send_email` must not assume message ordering: each received job is processed in its own spawned task, so jobs can run concurrently and complete out of order.
- `RetryEmail((email_id, hub_id))` is effectively idempotent per recipient: already-sent recipients (`recipient.is_sent == true`) are skipped.
- `NewEmail((user, new_email))` is not idempotent in this crate: it always inserts a new email row and recipients. If the upstream publisher may retry `NewEmail`, it must provide de-duplication at the source or switch to `RetryEmail` with a stable ID.

### Repository surface

Worker services depend on traits from `src/repository/mod.rs`:

- `EmailReader`
  - `get_email_by_id(email_id, hub_id) -> Option<EmailWithRecipients>`
  - `list_not_replied_email_recipients(hub_id) -> Vec<EmailRecipient>`
  - `get_email_recipient_by_id(recipient_id, hub_id) -> Option<EmailRecipient>`
- `EmailWriter`
  - `create_email(new_email) -> EmailWithRecipients`
  - `update_recipient(recipient_id, updates) -> EmailWithRecipients` (also recalculates email aggregate counters)
  - `unsubscribe_recipient(email, hub_id, reason) -> ()`
- `HubReader`
  - `get_hub_by_id(hub_id) -> Option<Hub>`
  - `list_hubs() -> Vec<Hub>`
- `HubWriter`
  - `set_imap_last_uid(hub_id, uid) -> ()`

### Mailer contract

`send_email::service` defines a `Mailer` trait (`src/send_email/service.rs`) used for dependency injection and tests:

- `send(&self, hub: &Hub, message: MessageBuilder<'_>) -> Result<(), Error>`

The production implementation (`src/send_email/mod.rs`) uses implicit TLS SMTP (`mail_send::SmtpClientBuilder::implicit_tls(true)`).

## Error Semantics

### Error taxonomy

All fallible operations use `crate::errors::Error` (`src/errors.rs`) with source conversions for:

- SMTP (`mail_send::Error`) → `Error::Smtp`
- IMAP (`async_imap::error::Error`) → `Error::Imap`
- ZeroMQ (`zmq::Error`, plus `pushkind_common::zmq::ZmqSenderError`)
- TLS (`tokio_rustls::rustls::Error`)
- Repository (`pushkind_common::repository::errors::RepositoryError`)
- DB pool construction (`diesel::r2d2::PoolError`)
- Configuration / validation issues (`Error::Config(String)`)

### Worker-level behavior

- `send_email` (`src/send_email/mod.rs`)
  - The main loop logs JSON parse errors and continues.
  - On a successfully parsed job, processing is moved to a spawned Tokio task; per-recipient SMTP failures are logged and do not fail the whole job.
  - Certain conditions become hard errors for the spawned task (e.g., invalid IDs, repository failures). A missing hub is logged and treated as a no-op for that job.
  - Transport-level ZMQ receive errors bubble out of the loop and terminate the worker process (the caller logs and exits).
- `check_reply` (`src/check_reply/mod.rs`)
  - One monitor task is spawned per hub returned by `list_hubs()` at startup.
  - Each hub monitor runs in a restart loop: configuration lookup failures, IMAP connection/auth failures, or IMAP idle errors are logged and retried after a short backoff.
  - Publishing `ZMQReplyMessage`/`ZMQUnsubscribeMessage` and persisting unsubscribes are best-effort: failures are logged but do not stop monitoring.

### Parsing failures

- Inbound parsing failures (`mailparse` errors, invalid reply text, invalid recipient ID extraction) are logged and skipped for that message/field; the hub monitor continues.
- Reply text is extracted from `text/plain` or `text/html` bodies (HTML is converted to text); quoted/original message sections are heuristically removed.

## Recipient state update rules

- Outbound (`send_email`)
  - For each `EmailRecipient` that is not yet sent, attempt SMTP delivery.
  - On successful SMTP send, persist `is_sent=true` via `EmailWriter::update_recipient`.
  - On SMTP failure, do not update recipient state.
- Inbound (`check_reply`)
  - If the inbound message contains a correlatable recipient ID (via `In-Reply-To: <{recipient_id}@{domain}>`) and that recipient exists in the hub, persist:
    - `is_sent=true` and `opened=true` (even if those flags were previously false).
    - `reply` set to the extracted reply text if it validates as `EmailRecipientReply`; invalid replies are ignored (but `opened=true` is still set).
  - If multiple replies are detected for the same recipient, later valid replies overwrite the stored `reply` value (no append/first-wins logic is implemented).
- Unsubscribes
  - Unsubscribe/bounce detection persists an unsubscribe record keyed by `(hub_id, email address)` and publishes `ZMQUnsubscribeMessage`.
  - Unsubscribe persistence does not currently mutate `EmailRecipient` rows directly in this crate.
