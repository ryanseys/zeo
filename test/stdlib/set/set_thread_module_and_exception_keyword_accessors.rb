# Set#reset, Thread.list, Module#const_set, and the exception accessors that
# take keyword/positional data: KeyError(key:/receiver:), SystemExit(status),
# LocalJumpError#reason, FrozenError#receiver.

require "set"
p Set[1, 2, 3].reset.to_a.sort
p Thread.list.all? { |t| t.is_a?(Thread) }
module Box; end
p Box.const_set(:X, 99)
p Box::X
ke = KeyError.new("m", key: :k, receiver: {1 => 2})
p [ke.key, ke.receiver, ke.message]
p [SystemExit.new(2).status, SystemExit.new(2).success?, SystemExit.new.success?, SystemExit.new(true, "bye").message]
s = "x".freeze
begin; s << "y"; rescue FrozenError => e; p e.receiver; end
def mm; yield; end
begin; mm; rescue LocalJumpError => e; p [e.reason, e.exit_value]; end
begin; {}.fetch(:z); rescue KeyError => e; p e.key; end
__END__
[1, 2, 3]
true
99
99
[:k, {1 => 2}, "m"]
[2, false, true, "bye"]
"x"
[:noreason, nil]
:z
