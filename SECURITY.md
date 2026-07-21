# Security Policy

## Reporting a vulnerability

Report privately via GitHub's security advisories ("Report a vulnerability"
on the repository's Security tab) or by email to ryan@ryanseys.com. Please
do not open public issues for suspected vulnerabilities.

## Scope notes

- Programs compiled by zeo execute with the full privileges of the invoking
  user, like any native binary; compiling untrusted Ruby source is equivalent
  to running it.
- `SecureRandom`/`OpenSSL::Random` draw from the operating system's CSPRNG.
  The `openssl` extension is a small pure-Rust subset -- it does not provide
  TLS or certificate verification today; treat any code path expecting real
  OpenSSL guarantees as unsupported (see docs/EXTENSIONS.md).
- `Kernel#rand`/`Random` are deterministic PRNGs and are NOT suitable for
  secrets (true in CRuby as well).

## Supported versions

Pre-1.0: only the `main` branch receives fixes.
