# Conformance scoreboard

Suite `spinel` — **2332/2336 passing (99.8%)** — oracle `ruby 4.0.5 (2026-05-20 revision 64336ffd0e) +PRISM [arm64-darwin25] [--disable-error_highlight --disable-did_you_mean]` — zeo `c64ebe6`

| verdict | count |
|---|---|
| PASS | 2332 |
| FAIL_OUTPUT | 3 |
| FAIL_COMPILE | 1 |
| FAIL_RUSTC | 0 |
| FAIL_RUN | 0 |
| TIMEOUT_COMPILE | 0 |
| TIMEOUT_RUN | 0 |
| ORACLE_FAIL | 0 |
| SKIP | 0 |
| **TOTAL** | **2336** |

## Top failure categories

| blocked | bucket | cluster | sample test | sample message |
|---|---|---|---|---|
| 1 | auto-from-users-ryanseys-dev-zeo-crates-xtask-conformance-e249 | ? | ffi_callback | from /Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/ffi_callback.rb:24:in '<main>' |
| 1 | auto-territory-not-syntaxerror-ed71 | ? | ffi_variadic | territory, not a SyntaxError) |

## Skipped tests

(none)

Regenerate with `cargo run -p xtask -- conformance run --update-scoreboard`.
