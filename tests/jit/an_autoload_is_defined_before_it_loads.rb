# `defined?` and `const_defined?` announce an `autoload`ed constant the moment
# the `autoload` is written -- and a NESTED ask loads the file to answer.
#
# zeo answered nil to both, and the consequence was not abstract:
# `rubygems/openssl.rb` is
#
#     autoload :OpenSSL, "openssl"
#     HAVE_OPENSSL = defined? OpenSSL::SSL
#
# so rubygems concluded it had no OpenSSL and refused every HTTPS source with
# "OpenSSL is not available. Install OpenSSL and rebuild Ruby".
#
# The two asks differ, which is the whole subtlety: a BARE `defined?(A)` says
# "constant" WITHOUT running the autoload, and `defined?(A::B)` runs it,
# because resolving the head is a real constant read.
#
# JIT-ONLY, and the reason is the AOT backend's own rule rather than this
# fix: the target here is named by a string the compiler cannot resolve, so
# no unit is built for it and an AOT binary has no run-time compiler linked
# to load it with. It says so plainly -- "this program was compiled without
# the unit compiler". rubygems is unaffected because ITS autoload targets
# are files the compiler already compiles as units; what it could not do
# before this fix was RUN one.

DIR = File.expand_path("../fixtures/autoload_target", __dir__)
$LOAD_PATH.unshift(DIR)

module Bare
  autoload :Widget, "autoload_widget"
end

puts "bare defined?\t#{defined?(Bare::Widget).inspect}"
puts "const_defined?\t#{Bare.const_defined?(:Widget)}"
puts "autoload?\t#{Bare.autoload?(:Widget).inspect}"

module Nested
  autoload :Deep, "autoload_deep"
end

# The head has not run yet, so nothing knows about `Deep::Inner`...
puts "before\t#{$autoload_deep_loaded.inspect}"
# ...and asking loads it.
puts "nested defined?\t#{defined?(Nested::Deep::Inner).inspect}"
puts "after\t#{$autoload_deep_loaded.inspect}"
puts "missing nested\t#{defined?(Nested::Deep::Absent).inspect}"

# A bare ask does NOT load. Fresh module, fresh target.
module Untouched
  autoload :Quiet, "autoload_quiet"
end
puts "bare ask\t#{defined?(Untouched::Quiet).inspect}"
puts "still unloaded\t#{$autoload_quiet_loaded.inspect}"

# Reading it for real is what runs it.
Untouched::Quiet
puts "after read\t#{$autoload_quiet_loaded.inspect}"
