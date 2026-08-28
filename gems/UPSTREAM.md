# Bundled gem provenance

Every directory here is a pure-Ruby gem Zeo compiles in when a program
`require`s it (resolved by `parse/loader.rs`; recorded `by: bundled-gem` in
each compile's `zeo-gems.json`). Three origins -- git-sourced, vendored from
the oracle's installation, and zeo-authored:

**Git-sourced.** 24 gems are fetched from their own upstream repository by
`tools/zeo-dev gem`, pinned in `upstream.rb` by `github`, release `tag`
and a full commit `rev` (the reproducible pin; `bundler` also carries a
`subdir`). Their versions live in each gem's own gemspec, their licenses in
each gem's own tree. Currently: `abbrev`, `benchmark`, `bundler`, `csv`,
`drb`, `erb`, `fileutils`, `find`, `ipaddr`, `logger`, `net-ftp`, `net-http`,
`net-protocol`, `net-smtp`, `observer`, `open3`, `racc`, `reline`, `resolv`,
`rubygems`, `tempfile`, `time`, `tmpdir`, `un`, `uri`.

**Faithful vendored copies** of the default/bundled gems shipping with the
oracle Ruby at the time of vendoring (4.0.5; the oracle now pins 4.0.6 in
`mise.toml`) -- copied verbatim from its installation, versions below, and
every one carrying its upstream license text (all are Ruby-license/2-clause
BSD or MIT, compatible with Zeo's MIT OR Apache-2.0).

A Ruby default gem that ships no license file of its own is covered by Ruby's
own dual license, so those dirs carry Ruby's `COPYING` + `BSDL` pair verbatim
(`delegate`, `English`, `forwardable`, `prime`, `shellwords`, `singleton`,
`weakref`). `minitest` is MIT and reproduces its license in the vendored
`README.rdoc`, exactly as upstream does.

| gem | version | source |
|---|---|---|
| delegate | 0.6.1 | ruby 4.0.5 stdlib |
| english (`English`) | 0.8.1 | ruby 4.0.5 stdlib |
| forwardable | 1.4.0 | ruby 4.0.5 stdlib |
| irb | 1.18.0 | ruby 4.0.6 bundled gem |
| minitest | 6.0.6 | ruby 4.0.6 bundled gem |
| optparse | 0.8.1 | ruby 4.0.6 default gem |
| ostruct | 0.6.3 | ruby 4.0.5 default gem |
| pp | 0.6.4 | upstream ruby/pp |
| prettyprint | 0.2.0 | ruby 4.0.5 default gem |
| prime | 0.1.4 | ruby 4.0.6 bundled gem |
| shellwords | 0.2.2 | ruby 4.0.5 stdlib |
| singleton | 0.3.0 | ruby 4.0.5 stdlib |
| timeout | 0.6.1 | upstream ruby/timeout |
| tsort | 0.2.0 | ruby 4.0.5 default gem |
| weakref | 0.1.4 | ruby 4.0.6 stdlib |

`optparse/` is incomplete: upstream ships eight files under `lib/optparse/`
and this copy has three, so `require "optparse/time"`, `"optparse/date"`,
`"optparse/uri"` and `"optparse/ac"` all fail. Resolving the gem from the
lock closes it.

`irb/` carries one removal: `lib/irb/ext/tracer.rb` is reduced to a
comment-only file. It hangs off the `tracer` gem, which ruby 4.0.6 does not
ship and Zeo does not vendor.

**The libraries zeo owns are NOT here.** Each is a gem-shaped directory
under `crates/zeo-rt/ext/`, its Ruby half beside the Rust that implements it:
`json`, `psych`, `openssl`, `zlib`, `strscan`, `nkf`, `syslog`, `socket`,
`pty`, `monitor`, `ffi`, `date`, `bigdecimal`, `prism`, `fiddle`. Their
provenance -- including which files are vendored and what deviates -- is in
`crates/zeo-rt/ext/UPSTREAM.md`.

## What is NOT here

Only libraries the reference Ruby itself ships. A `require` then reaches the
same code under both engines, and a golden that fails is a zeo bug.

Anything else -- `rspec` and its dependencies, `ffi`, `concurrent-ruby` --
would make zeo answer a require Ruby refuses, which is a divergence dressed as
a feature. Those come from `vendor/bundle` instead, the Gemfile.lock set
`make install-deps` resolves, and the goldens that need them read it. Both
engines do, so those goldens stay differentials rather than recordings.

`rspec-support/` used to carry a marked deviation here, probing for `ripper`
rather than inferring it from `RUBY_ENGINE`; the store's unpatched copy runs
correctly, so the deviation is gone with the vendored tree. `reline/` used to
carry one too, hoisting `require "reline/io/ansi"` out of a method body; the
loader reaches a method-body require on its own now, and reline 0.7.0 is
vendored verbatim.


## Choosing a version

Zeo tracks each gem's **latest upstream release**, not the version the oracle
Ruby happens to bundle. The stdlib gems release independently of Ruby itself,
and pinning to a Ruby release would freeze Zeo behind fixes its users want.

`tools/zeo-dev gem outdated` prints, per git-sourced gem, the current
pin beside two reference points: what the oracle install resolves, and the
newest upstream tag. Bump with `gem update <name> --tag vX.Y.Z` -- nothing
bumps automatically, so every move is deliberate.

Where a gem ends up ahead of the Ruby it is compared against, expect the
oracle to disagree about that gem's `VERSION`, and record the divergence in
`docs/COMPATIBILITY.md`. `bundler` and `rubygems` ship from one repository and
must move together, on a single shared `tag`/`rev` -- a test enforces it.

Reading a version off the install is easy to get wrong, which is how several
gems here drifted: `gem install` drops newer copies into the same store and
`require` activates the newest, so neither `gem list` nor a require-and-print
probe reports what Ruby actually shipped. For that, read
`<gemdir>/specifications/default/*.gemspec` (Ruby owns that directory), and
for bundled gems (`Gem::BUNDLED_GEMS::SINCE` names them) separate Ruby's
install batch from later user installs by gemspec mtime.

When bumping the oracle Ruby, re-check the first table against the new
installation and update the versions here.
