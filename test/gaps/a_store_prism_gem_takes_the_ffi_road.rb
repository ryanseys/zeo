# `require "prism"` out of the gem store answers without loading anything and
# without raising: `Prism::BACKEND` is `:CEXT`, `Prism::LibRubyParser` is
# undefined, `Init_prism` never runs, and `Prism.parse` is a NoMethodError.
# `crates/zeo/tests/fixtures/capi_sweep/XFAIL.json` carries the same row.
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
