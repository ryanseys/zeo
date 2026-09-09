# zeo

The compiler and CLI of [zeo](https://github.com/ryanseys/zeo), an
ahead-of-time Ruby compiler. It reads a whole program with Prism, lowers
it to Cranelift IR, and links the result against
[`zeo-rt`](https://crates.io/crates/zeo-rt), a Ruby runtime written in
Rust. The binary it writes needs no Ruby on the machine that runs it.

```console
$ cargo install zeo
$ zeo hello.rb          # compiles and runs, like `ruby hello.rb`
$ zeo -o hello hello.rb # writes a native binary
```

Building a binary needs a linker (`cc`) on the machine.

The crate is also a library: every `eval` in a compiled program calls back
into it, so the same compiler answers at run time.

Documentation, the compatibility ledger and the contributor guide live in
the [repository](https://github.com/ryanseys/zeo#documentation).
