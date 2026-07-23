# Conformance failures — full detail

Suite `spinel` — **3 failing test(s)** — oracle `ruby 4.0.5 (2026-05-20 revision 64336ffd0e) +PRISM [arm64-darwin25] [--disable-error_highlight --disable-did_you_mean]` — zeo `a294f49`

Every non-passing test with its source path, the reference it is diffed
against, verdict/bucket, captured stderr, and the full expected-vs-actual
diff — enough to understand each failure without re-running the harness.
Regenerate with `cargo run -p xtask -- conformance run --update-scoreboard`.

## `ffi_callback` — FAIL_OUTPUT

- source: `/Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/ffi_callback.rb`
- expected stdout: `/Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/ffi_callback.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_OUTPUT (stage `expect`) · bucket `auto-from-users-ryanseys-dev-zeo-crates-xtask-conformance-e249` (cluster `?`)

stderr:
```
/Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/ffi_callback.rb:11:in 'L.qsort': cannot convert Proc into an FFI pointer (TypeError)
	from /Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/ffi_callback.rb:24:in '<main>'
```

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (5 lines)
[1, 1, 2, 3, 4, 5, 6, 9]
[40, 30, 20, 10]
5
miss
done

--- actual (0 lines)

=== stderr diff ===
first difference at line 1
--- expected (0 lines)

--- actual (2 lines)
test/ffi_callback.rb:11:in 'L.qsort': cannot convert Proc into an FFI pointer (TypeError)
	from test/ffi_callback.rb:24:in '<main>'
```

---

## `ffi_variadic` — FAIL_COMPILE

- source: `/Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/ffi_variadic.rb`
- expected stdout: `/Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/ffi_variadic.rb.expected`
- expected stderr: *(must be empty)*
- verdict: FAIL_COMPILE (stage `compile`) · bucket `auto-territory-not-syntaxerror-ed71` (cluster `?`)

stderr:
```
 14 │     
    ╰────
  help: valid Ruby that zeo does not compile yet (NotImplementedError
        territory, not a SyntaxError)
```

diff:
```
=== stdout diff ===
first difference at line 1
--- expected (5 lines)
1-22-hi
8
3.50
[k=7]
plain

--- actual (0 lines)

=== stderr diff ===
first difference at line 1
--- expected (0 lines)

--- actual (1 lines)
uncaught exception: undefined method 'ffi_func' for module C (NoMethodError)
```

---

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

