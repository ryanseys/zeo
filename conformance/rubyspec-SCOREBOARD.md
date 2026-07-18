# Conformance scoreboard

Suite `rubyspec` — **3/67 passing (4.5%)** — oracle `ruby 4.0.5 (2026-05-20 revision 64336ffd0e) +PRISM [arm64-darwin25]` — spinel-rs `00a8a4d`

| verdict | count |
|---|---|
| PASS | 3 |
| FAIL_OUTPUT | 1 |
| FAIL_COMPILE | 62 |
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
| 33 | spike-misc | ? | alias_spec |
| 2 | splat | a | for_spec |
| 1 | auto-01a86947 | ? | numbered_parameters_spec |
| 1 | auto-083ceea9 | ? | precedence_spec |
| 1 | auto-1cd8ccca | ? | it_parameter_spec |
| 1 | auto-21cbb640 | ? | send_spec |
| 1 | auto-2564c2d5 | ? | retry_spec |
| 1 | auto-411aa863 | ? | heredoc_spec |
| 1 | auto-4e7163de | ? | break_spec |
| 1 | auto-51c70a78 | ? | metaclass_spec |

## Skipped tests

(none)

Regenerate with `cargo run -p xtask -- conformance run --update-scoreboard`.
