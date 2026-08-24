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
| `ZEO_CORELIB=ruby` | **default.** The vendored Ruby answers. |
| `ZEO_CORELIB=rust` | the Rust builtin answers, as before the corelib landed. |

The switch is read once per compile, and the whole golden corpus runs in both
modes. It exists so the two rows can be A/B'd and so a corelib bug has a way
back that is not a rebuild — not as a permanent fork. The Rust rows stay.

## What it costs, measured

`nilclass.rb` is 63 lines and 5 methods. Compiling it in costs **+33,856 bytes**
per binary (+0.22% on a hello) and no measurable compile time, and it closes
seven divergences: five `source_location` rows and two `Method#inspect`
renderings, including `rationalize(eps=...)` where zeo answered
`rationalize(*)`.

Bigger files are not free. Compiling `pathname_builtin.rb` (1,172 lines) costs
**+8.7 MB** against the +34 KB its Rust row costs today, because the Rust row
already lives in the shared archive while a Ruby row is emitted into every
program that uses it. That is zeo's ordinary rate for Ruby — `require
"optparse"` is +11.1 MB and `require "csv"` is +9.5 MB — so it is the price of
a Ruby row, not a corelib surcharge. It is why the segment table gates each
file rather than compiling the whole corelib in unconditionally.

## Adding a file

1. `tools/zeo-dev corelib census` — confirm it says `ELIGIBLE`.
2. Add its name to `upstream.rb`'s `corelib` entry.
3. `tools/zeo-dev corelib sync` — vendors it and re-locks.
4. Add a `Segment` to `crates/zeo/src/parse/corelib.rs` with CRuby's own
   `<internal:>` name for it.
5. Write a golden that reflects on the rows, and run the corpus in **both**
   `ZEO_CORELIB` modes.
