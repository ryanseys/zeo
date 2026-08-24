# The corelib: CRuby's own Ruby, compiled in

CRuby writes 24 of its core files in Ruby and compiles them into the
interpreter (`BUILTIN_RB_SRCS` in `common.mk`). A method defined in one of them
reports an `<internal:>` name rather than a path:

```console
$ ruby -e 'p NilClass.instance_method(:to_i).source_location'
["<internal:nilclass>", 36]
```

zeo vendors those files **verbatim** into `crates/zeo/corelib/` from the same
`ruby/ruby` pin the C API headers ride, and compiles them into the program
ahead of its first statement. The rows a program dispatches to are then CRuby's
own code, so the signatures, the `source_location`s and the corner cases agree
by construction instead of by a Rust reimplementation being kept in step.

## What is eligible

Only a file with **no** `Primitive.`, `__builtin` or `cexpr!` in it. Those are a
C-level escape hatch zeo has no answer for, and a file carrying one would
compile and then fail at the call. `zeo-dev corelib census` prints the whole
list with its primitive count:

```console
$ tools/zeo-dev corelib census
nilclass.rb                  63 lines  vendored
pathname_builtin.rb        1172 lines  ELIGIBLE
prelude.rb                   48 lines  ELIGIBLE
gem_prelude.rb               27 lines  ELIGIBLE
jit_hook.rb                  12 lines  ELIGIBLE
array.rb                    284 lines  needs 52 primitive(s)
...
```

Five of the 24 qualify at v4.0.6. `zeo-dev corelib sync` refuses to vendor a
file that carries a primitive, so the directory cannot drift into needing a
mechanism that does not exist.

## Provenance

`crates/zeo/corelib/` is upstream's bytes and carries **no patch series** —
unlike the C headers next door, which describe zeo's object layout rather than
ruby's behaviour. A corelib file is CRuby's own code or it is not corelib: a
local edit would make the claim this directory exists to make false.

Three checks hold it up, and each covers what the others cannot.

| Check | Needs | Proves |
|---|---|---|
| `crates/zeo/build.rs` | nothing | the bytes `include_str!` embeds are the bytes `upstream.lock` names |
| `tools/zeo-dev corelib verify` | nothing | the committed files match the lock — runs in CI, in a release tarball, offline |
| `tools/zeo-dev corelib sync --check` | network | the locked bytes **are** upstream's at the pinned rev |

`build.rs` is the load-bearing one. It hashes each file and stages it into
`OUT_DIR`, and the compiler includes the *staged* copy — so there is no second
path by which an edited or corrupted corelib file could be embedded silently. A
mismatch is a build failure naming the file:

```
corelib/nilclass.rb does not match upstream.lock
  locked 695e5391dea52fc8fd45511957595c4d07afd37e6d87d0b57307f3760a33beac
  actual f7364611e1b5cde66315b3a6eaf55f842a6ad7d5e173b97a9a4203b1180b141b
```

`upstream.lock` records two digests per file. The SHA-256 is the cryptographic
one. The **git blob OID** — `sha1("blob <len>\0" + bytes)` — is what GitHub's
contents API answers, so anyone can verify a vendored file against github.com
with one request and no clone:

```console
$ curl -s 'https://api.github.com/repos/ruby/ruby/contents/nilclass.rb?ref=03b6d3f8898a28604fe6cb00eae3226b821168f4' | jq -r .sha
acd5666c71d08d9ebf35e12e4ca90543d9930539
$ git hash-object crates/zeo/corelib/nilclass.rb
acd5666c71d08d9ebf35e12e4ca90543d9930539
```

## Choosing the row

| | |
|---|---|
| unset | each segment's own default — see below. |
| `ZEO_CORELIB=rust` | the Rust builtins answer, as before the corelib landed. |
| `ZEO_CORELIB=<names>` | exactly those segments. |

The switch is read once per compile, where the `CompileOptions` is built, and
the whole golden corpus runs with the defaults and with `rust`. It exists so
the two rows can be A/B'd and so a corelib bug has a way back that is not a
rebuild — not as a permanent fork. The Rust rows stay.

## Choosing segments

`ZEO_CORELIB` selects them:

| | |
|---|---|
| unset | each segment's own default |
| `rust` | every segment off — the Rust builtins throughout |
| `nilclass,pathname` | exactly those, whatever their defaults say |

A name that is not a segment is an error, not a silent no-op. The whole golden
corpus runs with the defaults and with `rust`.

## What it costs, measured

A Ruby row is emitted into every binary that can reach it; a Rust row already
lives in the shared archive. That is the only reason a segment is ever off, and
it is a size decision rather than a correctness one — both are CRuby's bytes.

`nilclass.rb` is 63 lines and 5 methods: **+17,328 bytes** (+0.18% on a hello)
and no measurable compile time. It closes seven divergences — five
`source_location` rows and two `Method#inspect` renderings, including
`rationalize(eps=...)` where zeo answered `rationalize(*)`. On by default.

`pathname_builtin.rb` is 1,172 lines and 94 methods. Two things stop it being
on by default, and both are filed:

- **One `eval` in it costs 14.18 MB.** `pathname_builtin.rb:273` is
  `eval("$~ = Thread.current[:pathname_sub_matchdata]", block.binding)`. Any
  `eval` sets `Hir::uses_runtime_eval`, which links the whole compiler —
  1,425 cranelift symbols — into the binary. Measured: a hello is 9,764,552
  bytes and `puts eval("1+1")` is 23,947,944, so the tax is 14,183,392 on any
  program at all. Turning pathname on costs 24,246,024 — that same tax plus
  about 300 KB of actual Ruby. The source is a LITERAL, so nothing about it
  needs a compiler at run time. This is not a gap file: it changes no
  program's output, only the artifact, so there is nothing for an XFAIL to
  compare. See "The literal-eval lever" below.
- **A reopened Rust class ignores a Ruby `initialize`.** `Pathname.new` runs
  the Rust constructor, so `@path` is never set and every Ruby method reads
  nil. Everything else about a reopen already works -- an overridden method,
  and a Ruby ivar on a String, Array, Hash, Struct, Exception or Object. The
  one broken row is `Class#new`, which calls a builtin's registered
  constructor unconditionally where ruby does `allocate` plus `initialize`.
  See `tests/gaps/a_reopened_rust_class_ignores_a_ruby_initialize.rb`.

Pathname is worth both: it holds 41 of the 46 remaining `Method#parameters`
divergences, and closing it deletes 1,034 lines of Rust.

For scale, ~300 KB is well under zeo's ordinary rate for Ruby — `require
"optparse"` is +16.96 MB and `require "csv"` is +15.36 MB, and both of those
are dominated by the same compiler-embed, because both libraries `eval`.

## Adding a file

1. `tools/zeo-dev corelib census` — confirm it says `ELIGIBLE`.
2. Add its name to `upstream.rb`'s `corelib` entry.
3. `tools/zeo-dev corelib sync` — vendors it and re-locks.
4. Add a `Segment` to `crates/zeo/src/parse/corelib.rs` with CRuby's own
   `<internal:>` name for it.
5. Write a golden that reflects on the rows, and run the corpus in **both**
   `ZEO_CORELIB` modes.

## The literal-eval lever

Any `eval` in a program links the whole compiler into its binary, and for
`pathname_builtin.rb` that is 14.18 MB against about 300 KB of actual Ruby.
The same tax falls on every library that evals: `require "optparse"` is
+16.96 MB and `require "csv"` is +15.36 MB for this reason and not for their
code.

**Why it is fixable.** `Hir::uses_runtime_eval` answers true for any `eval`
call, and a program that answers true carries `zeo_eval_install`, which is
what stops `-dead_strip` dropping cranelift and prism. But an eval compile is
parameterized by the caller's scope through exactly one input — `scope_names`,
which exists only to tell an identifier from a vcall, because a binding's
locals travel as shared **cells** rather than a baked frame layout. Everything
else the snippet needs (cref, `self`, the binding itself) is a run-time
**value** the entry already takes.

So a snippet that names no local is compilable at build time.

**The rule.** A literal source whose parse has no local read or write, no
`yield`/`super`/`block_given?`, and no `def`/`class`/`module` is
scope-agnostic. Compile it into the program as an ordinary body, key it by
source, and have the eval entry call it before reaching for `compiler()`. A
program whose every eval site is covered stops setting `uses_runtime_eval`.

This is **not** the retired literal-eval splice (`docs/EVAL.md`). That inlined
the snippet into the caller, which got the home wrong for `yield` and `super`
— and those are exactly the shapes the rule above excludes. The body is
*called* with the binding rather than spliced into it.

Until it lands, `pathname` stays off by default and no binary pays for it.
