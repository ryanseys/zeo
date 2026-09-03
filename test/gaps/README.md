# Programs zeo does not get right yet

Each file here is a Ruby program whose answer under `__END__` was recorded
from **ruby**, and which zeo does not currently match.

The verdict for this directory is inverted. A gap that still differs from
ruby is green; one that **matches** is red:

```
GAP FIXED: <name> now matches ruby.
Promote it: `cargo xtask promote-gap <name> <topic/area>`
```

So the day a gap starts working, the suite says so. That is the whole point
of running them.

To see where one stands, run it both ways:

```console
$ ruby test/gaps/<name>.rb
$ cargo run -p zeo -- test/gaps/<name>.rb
```

To promote a fixed one — the program, its recorded answer and its fixture
directory move together, and the promoted case is run once to confirm:

```console
$ cargo xtask promote-gap <name> core/string
```

A gap is bounded at 30 seconds rather than the ordinary 120: not matching is
the answer this directory wants, and a hang gives it as well as a crash does,
while making the whole suite wait for it.
