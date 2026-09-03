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

They go in `todo/`, with ruby's answer recorded, and **nothing runs them**.
The suite has one question — does this program match its recording — and no
second verdict for programs expected to fail. The trade is written down in
[`todo/README.md`](../../todo/README.md): a program there can start working
without anyone noticing.

When one starts matching, move it into the topic directory it belongs to and
it becomes a test like any other.
