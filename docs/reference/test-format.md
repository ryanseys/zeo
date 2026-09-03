# Test format

One `.rb` file is one test. The program is the file, and the answer is
recorded under `__END__` at the bottom of it. Ruby stops lexing at `__END__`,
so the recording costs the program nothing and can hold any bytes — binary
output, invalid UTF-8 — without escaping.

```ruby
#@ args: --seed 42
puts "ab".upcase
$stderr.puts "warn"
exit 3
__END__
AB
#@ stderr
warn
#@ exit 3
```

## Header directives

A directive is a line starting `#@` at column 0, anywhere above `__END__`,
in any order. An unknown key fails the case rather than being ignored.

| Directive | Meaning |
|---|---|
| `#@ args: <words>` | ARGV, shell-split, for both engines |
| `#@ stdin: <path>` | bytes piped to both engines; the path is relative to the program's own directory |
| `#@ env: K=V ...` | environment for both engines |
| `#@ ruby: <flags>` | flags for the oracle only |
| `#@ zeo: <flags>` | flags for the zeo CLI only, before the file name |
| `#@ zeo-env: K=V ...` | environment for the zeo child only |
| `#@ only: macos` / `#@ only: linux` | skip on every other platform |
| `#@ backend: jit` | skip wherever a real link is used; the program needs the compiler and itself in one process |
| `#@ gccheck: <line>` | the exit cycle census this program is expected to report |

## The trailer

Everything after a line that is exactly `__END__`.

The stdout body starts on the next line and runs to the next marker or to the
end of the file. A marker is a line at column 0 that is exactly one of:

| Marker | Meaning |
|---|---|
| `#@ stderr` | what follows is stderr |
| `#@ exit N` | the exit status |
| `#@ signal N` | the signal that killed it |
| `#@ nonl` | the body before this has no trailing newline |
| `#@ linux stdout` / `#@ linux stderr` / `#@ linux exit N` | the answer on linux; the unprefixed sections are macOS's |

`#@ exit` is omitted when the status is 0. `#@ stderr` is omitted when stderr
is empty, and its absence means stderr **must** be empty.

A body line that would look like a marker is escaped: the writer prefixes `#`
to any line matching `^#+@`, and the reader strips one `#` from any line
matching `^##+@`. So any output round-trips.

## Two rules the corpus enforces

- A program may not reference `DATA`. Ruby would hand it the trailer.
- A program may not read `__FILE__` as a file, for the same reason.

`checks::corpus_hygiene` holds both.

## Where a program goes

| Directory | What lives there | Who recorded the trailer |
|---|---|---|
| `test/lang/<area>/` | language semantics | ruby |
| `test/core/<class>/` | core classes | ruby |
| `test/stdlib/<lib>/` | bundled libraries | ruby |
| `test/compiler/<area>/` | zeo-specific behaviour | ruby |
| `test/aot/` | programs that also run through a real link | ruby |
| `test/errors/` | programs zeo must reject | zeo |
| `test/features/` | what a build of zeo answers about itself | zeo |
| `test/divergences/` | programs zeo answers differently on purpose | zeo |
| `test/milestones/` | whole require graphs; the `full` profile only | ruby |
| `test/ze0/` | the Ruby front end's subset, three roads | ruby |
| `test/bench/` | the benchmark bank's inputs | ruby |
| `test/gaps/` | programs zeo does not get right yet; the verdict is inverted | ruby |

The four topic roots are exactly two levels deep: `<topic>/<area>/<name>.rb`.
Anything deeper is a fixture, not a case. A directory named `<name>/` beside
`<name>.rb` holds that program's own files, and a `fixtures/` directory holds
shared ones.

A case is named `<suite>::<path>` — `core::string/upcase.rb` — which is the
spelling nextest reports and the one every filter takes.
