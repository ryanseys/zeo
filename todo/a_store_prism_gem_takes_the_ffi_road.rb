# `require "prism"` out of the gem store reaches `prism/ffi.rb` even though
# zeo builds and loads the gem's C extension: the DEBUG log shows
# `load C extension .../ext/prism/prism.bundle`, and `Prism.parse` still
# raises `uninitialized constant Prism::LibRubyParser::PrismString`.
#
# `prism` is the one gem in the C-API sweep with two backends, and its
# loader picks between them with `RUBY_ENGINE == "ruby"`, which zeo answers
# `"ruby"`. Recorded as a note; `crates/zeo/tests/fixtures/capi_sweep/
# XFAIL.json` carries the same finding for the sweep.
#@ zeo-env: ZEO_DISABLE_BUILTIN=prism
#@ zeo: --gem-path vendor/gems --bundle-gemfile Gemfile
require "prism"

r = Prism.parse("1 + 2\n")
puts r.success?, r.value.class
puts Prism::VERSION
__END__
true
Prism::ProgramNode
1.9.0
