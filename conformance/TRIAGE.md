# Gap triage

Failing tests grouped by normalized failure message, ranked by how many
tests each gap blocks. Clusters refer to the implementation plan's gap
families. Oracle `ruby 4.0.5 (2026-05-20 revision 64336ffd0e) +PRISM [arm64-darwin25] [--disable-error_highlight --disable-did_you_mean]`.

| cluster | bucket | blocked | sample tests | sample message |
|---|---|---|---|---|

List one bucket's tests: `cargo run -p xtask -- conformance triage --bucket <name>`.
