# Programs zeo does not get right yet

Each file here is a Ruby program whose answer under `__END__` was recorded
from **ruby**, and which zeo does not currently match. They are notes, not
tests: nothing compiles or runs them, and the suite does not go red for one.

To see where a program stands, run it both ways:

```console
$ ruby todo/<name>.rb
$ cargo run -p zeo -- todo/<name>.rb
```

When one starts matching, move it into the topic directory it belongs to
under `test/` and it becomes a test like any other:

```console
$ git mv todo/<name>.rb test/core/string/<name>.rb
$ cargo xtask bless core::string/<name>.rb
```

The trade this records: a program here can start working without anyone
noticing. That is the price of keeping the harness to one question --
"does this program match its recording" -- with no second verdict for
programs expected to fail.
