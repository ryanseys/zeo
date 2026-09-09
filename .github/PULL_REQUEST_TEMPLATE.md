<!--
The one rule: keep every difference from Ruby visible. CONTRIBUTING.md has
the conventions; this list is the short version.
-->

## What this changes

<!-- One or two sentences. The body of the commit message is the place for
     why; this is the place for what. -->

## Checks

- [ ] `cargo nextest run` is green.
- [ ] `cargo xtask check` is green (clippy at zero warnings included).
- [ ] New behaviour is verified against real `ruby`, as a program under
      `test/` in this same change.
- [ ] Any intentional divergence from Ruby has a comment at the code site
      saying what differs and why.
- [ ] A divergence a user can observe also has a row in
      [Compatibility](../docs/reference/compatibility.md), and a program
      under [`test/gaps/`](../test/gaps) when it is not deliberate.

<!-- A performance claim needs a fresh measurement beside it. Performance is
     not a gate: see docs/how-to/measure-performance.md. -->
