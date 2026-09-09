# Measure performance

What a compiled program costs to run, measured against the pinned ruby on the
same machine.

## The benchmark bank

```console
$ cargo bench -p zeo --bench programs
```

`test/bench/` holds the bank: 61 programs, each with its recorded answer.
The harness verifies the output before it times anything, so a benchmark
cannot get faster by being wrong.

```console
$ cargo bench -p zeo --bench programs -- 'zeo/bm_fib$'      # one benchmark
$ cargo bench -p zeo --bench programs -- --save-baseline before
$ cargo bench -p zeo --bench programs -- --baseline before  # compare
```

To compare two saved baselines after the fact, rather than against the last
run, use [`critcmp`](https://github.com/BurntSushi/critcmp):

```console
$ critcmp before after
```

Two groups share the bank: `zeo/` times the compiled binary, `ruby/` times
the same program under the pinned ruby, so the ratio is measured rather than
remembered.

The shipped configuration is profile-guided:

```console
$ ZEO_BENCH_DIST=pgo cargo bench -p zeo --bench programs
```

The current numbers, and what the losses are waiting on, are in
[Performance](../explanation/performance.md).

## Before you optimize anything

Measure first, on the machine you are on. The committed numbers were taken
on one machine on one day and both are stated beside them. Performance is
not a gate here: no test fails because something got slower, and a claim
without a fresh measurement beside it is not a claim.
