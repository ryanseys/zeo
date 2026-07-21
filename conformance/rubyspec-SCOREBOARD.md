# Conformance scoreboard

Suite `rubyspec` — **6/67 passing (9.0%)** — oracle `ruby 4.0.5 (2026-05-20 revision 64336ffd0e) +PRISM [arm64-darwin25]` — spinel-rs `cc13e39`

| verdict | count |
|---|---|
| PASS | 6 |
| FAIL_OUTPUT | 12 |
| FAIL_COMPILE | 44 |
| FAIL_RUSTC | 1 |
| FAIL_RUN | 4 |
| TIMEOUT_COMPILE | 0 |
| TIMEOUT_RUN | 0 |
| ORACLE_FAIL | 0 |
| SKIP | 0 |
| **TOTAL** | **67** |

## Top failure categories

| blocked | bucket | cluster | sample test | sample message |
|---|---|---|---|---|
| 29 | spike-misc | ? | file_spec | /Users/ryanseys/dev/spec/language/file_spec.rb: /Users/ryanseys/dev/spec/fixtures/code_loading.rb: `require` is only supported as a top-level statement with a single string-literal argument (spike scope) -- it's resolved at compile time, so it can't appear inside a method, block, conditional, `begin`, or `eval` body |
| 6 | missing-method:raise | P | or_spec | ERROR: The or operator has a lower precedence than 'return' in 'return true or false': NoMethodError: undefined method 'raise' for an instance of MSpecShould |
| 2 | auto-eval-unsupported-syntax-in-this-build-the-eval-dfcc | ? | encoding_spec | uncaught exception: eval: unsupported syntax in this build (the eval VM does not yet cover this node) (NotImplementedError) |
| 2 | missing-const:ScratchPad | P | BEGIN_spec | ERROR: The BEGIN keyword returns the top-level script's filename for __FILE__: NameError: uninitialized constant ScratchPad |
| 2 | toplevel-node-in-expr | ? | private_spec | internal error: unexpected top-level-only node in expression position |
| 1 | auto-fail-can-be-redefined-and-receive-frozen-string-9423 | ? | execution_spec | FAIL: %x can be redefined and receive a frozen string as argument: expected false == true |
| 1 | auto-fail-the-until-modifier-with-begin-end-block-c646 | ? | until_spec | FAIL: The until modifier with begin .. end block restart the current iteration without reevaluating condition with redo: expected [1, 1, 1, 2] == [0, 0, 0, 1, 2] |
| 1 | auto-fail-the-while-modifier-with-begin-end-block-6764 | ? | while_spec | FAIL: The while modifier with begin .. end block restarts the current iteration without reevaluating condition with redo: expected [1, 1, 1, 2] == [0, 0, 0, 1, 2] |
| 1 | auto-it-behaves-like-mspecliteunsupported-cd63 | ? | END_spec | uncaught exception: it_behaves_like (MSpecLiteUnsupported) |
| 1 | auto-users-ryanseys-dev-spec-language-array-spec-rb-e598 | ? | array_spec | /Users/ryanseys/dev/spec/language/array_spec.rb: unsupported syntax at "\"foo\" => :bar, baz: 42" (spike handles only what the 7 example programs need) |

## Skipped tests

(none)

Regenerate with `cargo run -p xtask -- conformance run --update-scoreboard`.
