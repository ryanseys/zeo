# The corpus

One `.rb` file is one test. The program is the file; the answer is recorded
under `__END__` at the bottom of it, by real ruby.

```ruby
# `upcase` on an ASCII string, and on one that is not.
puts "ab".upcase
puts "ä".upcase
__END__
AB
Ä
```

Run it:

```console
$ cargo nextest run
$ cargo nextest run -E 'test(core::string/upcase.rb)'
```

Record an answer:

```console
$ cargo xtask bless core::string/upcase.rb
```

## Where a program goes

| Directory | What lives there |
|---|---|
| `lang/<area>/` | language semantics |
| `core/<class>/` | core classes |
| `stdlib/<lib>/` | bundled libraries |
| `compiler/<area>/` | behaviour only zeo has |
| `aot/` | programs that also run through a real link |
| `errors/` | programs zeo must reject |
| `features/` | what a build of zeo answers about itself |
| `divergences/` | programs zeo answers differently on purpose |
| `milestones/` | whole require graphs (the `full` profile) |
| `bench/` | the benchmark bank's inputs, run once as goldens under `-P full` |
| `fixtures/` | files several programs share |

The four topic roots are exactly two levels deep. Anything deeper is a
fixture. A directory named `<name>/` beside `<name>.rb` holds that program's
own files.

Programs run with `test/` as their working directory, so a path in a
recording reads `core/string/upcase.rb`.

A program that writes files puts them under `Dir.mktmpdir` (`require
"tmpdir"`) and never prints the directory. Both ruby and zeo run it under
their own scratch root, so a literal `/tmp/name` would be shared with every
case running beside it and would outlive the run.

A bare relative name is the same mistake: the working directory is `test/`
itself, so `File.write("out.txt", …)` writes into the corpus. Where the
recorded answer names the file — an errno message does — make the scratch
directory the working directory (`Dir.chdir(Dir.mktmpdir)`) and keep the
bare name.

## The rest

- [The test format](../docs/reference/test-format.md) — directives, the
  trailer, and the escaping rules.
- [Add a test](../docs/how-to/add-a-test.md) — including the three
  directories that record zeo's answer instead of ruby's.
- [Testing](../docs/explanation/testing.md) — why it is built this way.
- [`gaps/`](gaps) — programs zeo does not get right yet. These run, and
  the suite goes red the day one starts matching ruby.
