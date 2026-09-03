# GAP -- imported from the spinel corpus at fa06b601.
#
# A builtin CLASS method's declared arity is wider than CRuby's, so a call
# CRuby rejects is accepted and the ArgumentError names the wrong range:
# zeo reports `expected 1..4` where CRuby says `1..3`.
#
# One row the arity census gated before it was retired at zero.
#
# The original header follows. It describes the PREDECESSOR project's
# version of this test and its own fix, not zeo's divergence above.
#
# A builtin class/module method called with a count CRuby rejects raises
# CRuby's ArgumentError. File.open with no arguments crashed the compiler.
def check
  yield
rescue => e
  p [e.class, e.message]
end

check { File.open }
check { File.new }
check { File.read }
check { File.write("only-a-path") }
check { IO.for_fd }
check { IO.sysopen }
check { Time.at }
check { Hash.new(1, 2) }
check { Integer.sqrt }
check { Math.lgamma }
check { Dir.mkdir }
# the valid shapes still answer
p Hash.new(5)[:missing]
# A name no run ever creates: this corpus's own working directory is `tests/`,
# so anything plausible there is a file somebody may add.
p File.exist?("no-such-file-e7c1a0")
__END__
[ArgumentError, "wrong number of arguments (given 0, expected 1..3)"]
[ArgumentError, "wrong number of arguments (given 0, expected 1..3)"]
[ArgumentError, "wrong number of arguments (given 0, expected 1..4)"]
[ArgumentError, "wrong number of arguments (given 1, expected 2..3)"]
[ArgumentError, "wrong number of arguments (given 0, expected 1..2)"]
[ArgumentError, "wrong number of arguments (given 0, expected 1..3)"]
[ArgumentError, "wrong number of arguments (given 0, expected 1..3)"]
[ArgumentError, "wrong number of arguments (given 2, expected 0..1)"]
[ArgumentError, "wrong number of arguments (given 0, expected 1)"]
[ArgumentError, "wrong number of arguments (given 0, expected 1)"]
[ArgumentError, "wrong number of arguments (given 0, expected 1..2)"]
5
false
