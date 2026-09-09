# An `autoload` names a NAMESPACE far more often than it names the leaf a
# program reads: net/http writes `autoload :OpenSSL, "openssl"` and then reads
# `OpenSSL::SSL::SSLContext`. The read has to run the target, so the gate at
# the fold has to fire on every PREFIX of the path, not on the whole of it.
#
# Keying only on the whole path never fired here, and the read found a class
# whose unit had not run -- `uninitialized constant OpenSSL::SSL::SSLContext`
# from inside `Net::HTTP#connect`. Caught by an HTTPS request, not by the
# corpus, which is why this shape is a golden now.
$LOAD_PATH.unshift(File.expand_path("a_nested_read_under_an_autoload_runs_the_target", __dir__))

puts "before"
autoload :NS, File.expand_path("a_nested_read_under_an_autoload_runs_the_target/ns", __dir__)
puts "after the declaration"

# Nothing has run yet.
p Object.autoload?(:NS)&.end_with?("ns")

# A read three levels down the namespace must run it.
puts "first read"
p NS::Inner::Deep.hi
p NS::LOADED
p Object.autoload?(:NS)
__END__
before
after the declaration
true
first read
  ns.rb body ran
"deep"
true
nil
