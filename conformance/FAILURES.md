# Conformance failures — full detail

Suite `spinel` — **1 failing test(s)** — oracle `ruby 4.0.5 (2026-05-20 revision 64336ffd0e) +PRISM [arm64-darwin25] [--disable-error_highlight --disable-did_you_mean]` — zeo `b50cd4d`

Every non-passing test with its source path, the reference it is diffed
against, verdict/bucket, captured stderr, and the full expected-vs-actual
diff — enough to understand each failure without re-running the harness.
Regenerate with `cargo run -p xtask -- conformance run --update-scoreboard`.

## `regexp_inline_options` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/regexp_inline_options.rb`
- expected stdout: `/Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/regexp_inline_options.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `-` (cluster `-`)

diff:
```
=== stdout diff ===
first difference at line 16
--- expected (30 lines)
1
0
nil
0
nil
0
nil
0
nil
0
nil
0
nil
0
0
ws-x: RegexpError
*: RegexpError
+: RegexpError
?: RegexpError
a***: RegexpError
(?~foo): RegexpError
(?(a)b): RegexpError
(?q)a: RegexpError
(?x:ab): compiled
2
nil
1
3
"いうえ"
nil

--- actual (30 lines)
1
0
nil
0
nil
0
nil
0
nil
0
nil
0
nil
0
0
ws-x: compiled
*: RegexpError
+: RegexpError
?: RegexpError
a***: compiled
(?~foo): compiled
(?(a)b): compiled
(?q)a: RegexpError
(?x:ab): compiled
2
nil
1
3
"いうえ"
nil
```

---

