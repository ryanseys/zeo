# `require "prism"` out of the gem store reaches `prism/ffi.rb` even though
# zeo builds and loads the gem's C extension: the DEBUG log shows
# `load C extension .../ext/prism/prism.bundle`, and `Prism.parse` still
# raises `uninitialized constant Prism::LibRubyParser::PrismString`.
#
# zeo takes the C-extension branch of `prism.rb` correctly, but `ENV` decides
# that branch, so the compile also takes in `prism/ffi.rb`, and with it the
# store's ffi, rake, debug, irb and rdoc: the compile alone runs past 30
# seconds. The `Prism.parse` call site binds to ffi.rb's static def rather
# than the one `Init_prism` defines at run time. `crates/zeo/tests/fixtures/
# capi_sweep/XFAIL.json` carries the same finding for the sweep.
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
