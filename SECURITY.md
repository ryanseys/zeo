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
- The `openssl` extension binds a **vendored OpenSSL 3.x** through the
  official rust-openssl bindings, so the digest, cipher, BN and TLS
  primitives are the ones CRuby binds. Client-side TLS verifies certificates:
  `SSLContext` honours `verify_mode`, `ca_file`/`ca_path` and the system
  trust store. As in CRuby, a bare `SSLContext.new` starts at `VERIFY_NONE`
  -- net/http raises it to `VERIFY_PEER`, but code that builds its own
  context must set it. PKey generation, X509 issuance, PKCS#7, ASN1 and
  `SSLServer` are declined; see docs/EXTENSIONS.md and docs/COMPATIBILITY.md.
- **OpenSSL links statically into every compiled binary.** An OpenSSL
  security fix reaches a program only when you rebuild it with an updated
  zeo. Programs already shipped keep the version they were compiled with.
- `Kernel#rand`/`Random` are deterministic PRNGs and are NOT suitable for
  secrets (true in CRuby as well).

## Supported versions

Pre-1.0: only the `main` branch receives fixes.
