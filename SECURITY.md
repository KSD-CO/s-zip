# Security Policy

`s-zip` handles untrusted archive input (parsing) and implements AES-256/192/128
encryption for password-protected ZIP entries. We take security issues seriously
and appreciate responsible disclosure.

## Supported Versions

Only the latest published minor version on [crates.io](https://crates.io/crates/s-zip)
receives security fixes. Please upgrade before reporting an issue to confirm it
still reproduces.

| Version | Supported          |
| ------- | ------------------ |
| 0.15.x  | :white_check_mark: |
| < 0.15  | :x:                |

## Reporting a Vulnerability

**Do not open a public GitHub issue for security vulnerabilities.**

Instead, report privately using one of the following:

- [GitHub Security Advisories](https://github.com/KSD-CO/s-zip/security/advisories/new) (preferred)
- Email: **ttvuhm@gmail.com** — please include `[SECURITY]` in the subject line

Include as much detail as possible:

- Affected version(s) and enabled feature flags (e.g. `encryption`, `async`, `cloud-s3`)
- A minimal reproducible example (crafted ZIP file, code snippet, or PoC)
- Impact (e.g. panic/DoS, memory exhaustion, incorrect decryption, path traversal)

We aim to acknowledge reports within **5 business days** and to ship a fix or
mitigation within **30 days** for confirmed issues, depending on severity.

## Scope

In scope:

- Memory-safety issues, panics, or resource exhaustion (DoS) when parsing
  untrusted/malformed ZIP archives (`reader.rs`, `async_reader.rs`, `format.rs`)
- Cryptographic flaws in the AES-256/192/128 + HMAC-SHA1 WinZip-compatible
  encryption implementation (`encryption.rs`, `decrypt_reader.rs`) — e.g. keystream
  reuse, incorrect authentication, timing side-channels in password verification
- Path traversal ("zip-slip") when extracting entries via `safe_path()`
- Unsound `unsafe` code (`src/cloud/s3.rs`, `src/parallel.rs`)

Out of scope:

- Vulnerabilities in third-party dependencies (report upstream; we track advisories
  via `cargo audit` in CI)
- Issues requiring a malicious/compromised password or a fully trusted local attacker
  with equivalent privileges to the running process
- Denial of service from resource limits the caller explicitly configured
  (e.g. an intentionally large `max_concurrent` in `ParallelConfig`)

## Known Limitations

- WinZip AES encryption authenticates ciphertext via HMAC-SHA1 (per the WinZip AES
  spec) but does not authenticate the ZIP metadata (filenames, sizes) outside the
  encrypted entry — this is a limitation of the format itself, not implementation-specific.
- `read_entry_streaming()` for encrypted entries only verifies the HMAC after all
  bytes have been consumed via `finish()`. Callers that abort early before calling
  `finish()` will not get authentication guarantees on the bytes already processed.
- The optional `cloud-s3` feature pulls in `aws-smithy-http-client`, which bundles a
  legacy hyper 0.14 / rustls 0.21 TLS backend alongside its primary hyper 1.x / rustls
  0.23 stack. This causes 4 known RUSTSEC advisories (RUSTSEC-2026-0258, -0104, -0098,
  -0099) to appear as transitive dependencies. No fix is currently available via
  `cargo update` alone; these are explicitly ignored in CI (`cargo audit` / `deny.toml`)
  with tracking comments to re-check when `aws-sdk-s3`/`aws-config` are upgraded.

## Disclosure Policy

Once a fix is released, we will publish a `RUSTSEC` advisory (via
[rustsec/advisory-db](https://github.com/rustsec/advisory-db)) for issues affecting
released versions, and credit the reporter (unless anonymity is requested).
