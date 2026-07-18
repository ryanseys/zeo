# Conformance scoreboard

Suite `rubyspec` — **4/67 passing (6.0%)** — oracle `ruby 4.0.5 (2026-05-20 revision 64336ffd0e) +PRISM [arm64-darwin25]` — spinel-rs `ddde250`

| verdict | count |
|---|---|
| PASS | 4 |
| FAIL_OUTPUT | 1 |
| FAIL_COMPILE | 61 |
| FAIL_RUSTC | 0 |
| FAIL_RUN | 1 |
| TIMEOUT_COMPILE | 0 |
| TIMEOUT_RUN | 0 |
| ORACLE_FAIL | 0 |
| SKIP | 0 |
| **TOTAL** | **67** |

## Top failure categories

| blocked | bucket | cluster | sample test |
|---|---|---|---|
| 49 | spike-misc | ? | BEGIN_spec |
| 2 | splat | a | for_spec |
| 1 | auto-619a71ee | ? | block_spec |
| 1 | auto-72ea658f | ? | module_spec |
| 1 | auto-80fe6faf | ? | execution_spec |
| 1 | auto-ba2b7615 | ? | precedence_spec |
| 1 | auto-bcd16e8b | ? | assignments_spec |
| 1 | auto-bf7964ee | ? | undef_spec |
| 1 | auto-e0dfb590 | ? | defined_spec |
| 1 | auto-f0c6b32f | ? | END_spec |

## Skipped tests

(none)

Regenerate with `cargo run -p xtask -- conformance run --update-scoreboard`.
