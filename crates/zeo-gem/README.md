# zeo-gem

RubyGems' file formats in Rust, for
[zeo](https://github.com/ryanseys/zeo), an ahead-of-time Ruby compiler.

zeo reads what Bundler and RubyGems already wrote. It never resolves a
dependency graph and never runs Ruby to find out what a gem holds. This crate
is the reader, and the writer for the two files a store needs:

| Module | Format |
|---|---|
| `version` | `Gem::Version` and `Gem::Requirement`, with RubyGems' ordering |
| `platform` | `Gem::Platform`, and which gem runs on which machine |
| `lockfile` | `Gemfile.lock`: every section, round-trip byte-exact |
| `gemspec` | a gemspec, parsed statically -- no Ruby is evaluated |
| `package` | a `.gem` file: metadata, checksums, and its files |
| `store` | a store on disk: `gems/` and `specifications/` |

There is no HTTP client here. Which gem to fetch is the lockfile's answer, and
fetching it is the caller's business, so a download tool picks its own
transport.

A gemspec is Ruby source, but the ones a store holds are written by
`Gem::Specification#to_ruby`, whose grammar is closed and tiny. This crate
parses that grammar with prism and refuses anything outside it, so reading a
gem never evaluates code that came off a server.

You do not depend on this crate directly. Install the
[`zeo`](https://crates.io/crates/zeo) CLI instead.

## License

MIT OR Apache-2.0.
