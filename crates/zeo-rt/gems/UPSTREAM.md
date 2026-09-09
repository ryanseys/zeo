# The pure-Ruby gem tier

One directory per gem zeo ports to pure Ruby, in the shape upstream ships:

```text
<name>/
  <name>.gemspec       zeo-authored, at upstream's version
  lib/<name>.rb        zeo's port
  test/                upstream's suite, verbatim, plus a driver
```

`crates/zeo/src/gems/bundled.rs` (`PURE_TIER`) is what makes a `require`
find them.

## What is zeo's, and what is not

The `lib/` trees and both gemspecs are zeo-authored. `strscan/lib/strscan.rb`
follows the structure of ruby/strscan's own `lib/strscan/truffleruby.rb`,
which is the same job done for another engine.

These are upstream's, verbatim:

| File | From |
|---|---|
| `stringio/test/test_stringio.rb` | ruby/stringio v3.2.0, test/stringio/test_stringio.rb |
| `stringio/ruby/ut_eof.rb` | ruby/ruby, test/ruby/ut_eof.rb |
| `strscan/test/test_stringscanner.rb` | ruby/strscan v3.1.8, test/strscan/test_stringscanner.rb |

Both are under ruby's own dual licence, the Ruby License or 2-clause BSD;
`THIRD-PARTY-NOTICES.md` carries the map.

## Why the suite is vendored at all

A port is only worth having if it answers what the C extension answers, and
the only fair judge of that is the extension's own tests. Each directory's
`run_pure.rb` puts `lib/` ahead of the store on `$LOAD_PATH`, checks that the
pure tree actually won the require, and runs upstream's suite against it.
`crates/zeo/tests/suite/api/pure_gems.rs` drives both.

The versions here are the ones `Gemfile.lock` pins, so the suite and the port
cannot drift apart. [Update the gem versions](../../../docs/how-to/update-gem-versions.md)
is the procedure when the lock moves.
