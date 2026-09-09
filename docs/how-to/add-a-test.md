# Add a test

Write the program, put it where it belongs, record what ruby says.

## The program

One `.rb` file. A header comment says what the program is about; the answer
goes under `__END__`. The full grammar is in
[the test format reference](../reference/test-format.md).

```ruby
# `upcase` on an ASCII string, and on one that is not.
puts "ab".upcase
puts "ä".upcase
__END__
AB
Ä
```

Two rules the harness enforces rather than trusts. The name says what the
program checks, not which issue it came from: a tracker id in a filename
tells a red board nothing. And a program that writes files puts them under
`Dir.mktmpdir` (`require "tmpdir"`) and never prints the directory — ruby
and zeo each run it under their own scratch root, so a literal `/tmp/name`
is shared with every case beside it.

## Where it goes

The topic directory the question belongs to:

| Question | Directory |
|---|---|
| a core class | `test/core/<class>/` |
| a bundled library | `test/stdlib/<lib>/` |
| language semantics | `test/lang/<area>/` |
| something only zeo has | `test/compiler/<area>/` |

Programs run with `test/` as their working directory, so a backtrace in a
recording reads `core/string/upcase.rb`.

If the program needs files of its own, put them in a directory named after
it: `core/string/upcase/` beside `core/string/upcase.rb`. Shared fixtures go
in a `fixtures/` directory.

## Record the answer

```console
$ cargo xtask bless core::string/upcase.rb
```

See [Record an answer](record-an-answer.md) for what `bless` needs.

## Programs that are supposed to differ

Three directories record **zeo's** answer rather than ruby's, because ruby's
is not the standard being held to:

- `test/errors/` — programs zeo must reject. The recording is the rejection.
- `test/divergences/` — programs where zeo answers differently on purpose.
  Each one says why in its header comment.
- `test/features/` — what a build of zeo answers about itself.

These need a built `target/release/zeo` (or `ZEO_BIN`) rather than ruby.

## Programs zeo does not get right yet

They go in `test/gaps/`, with ruby's answer recorded. They run like any other
case, but the verdict is inverted: still differing is green, and matching
fails with "GAP FIXED, promote it", so the day a gap starts working the suite
says so.

When one starts matching, promote it — the program, its recorded answer and
its fixture directory move together, and the promoted case is run once to
confirm:

```console
$ cargo xtask promote-gap <name> core/string
```
