# Carried-state introspection accessors: KeyError#key/#receiver,
# NameError/NoMethodError#receiver/#args/#private_call?, FrozenError#receiver,
# LocalJumpError#reason/#exit_value, UncaughtThrowError#tag/#value,
# and SystemExit (.new semantics, rescuable exit, #status/#success?).
h = {"a" => 1}
begin; h.fetch("zz"); rescue KeyError => e; p e.key; p e.receiver; end
h0 = {}
begin; h0.fetch(:x); rescue KeyError => e; p e.key; end
ke = KeyError.new("m", key: :k, receiver: {1 => 2})
p ke.key
p ke.receiver
p ke.message
begin; nil.foo(1, 2); rescue NoMethodError => e; p e.receiver; p e.private_call?; end
def m; yield; end
begin; m; rescue LocalJumpError => e; p e.reason; p e.exit_value; end
r = (begin; catch(:a) { throw :b }; rescue UncaughtThrowError => e; e.tag; end)
p r
r2 = (begin; catch(:a) { throw :b, 7 }; rescue UncaughtThrowError => e; e.value; end)
p r2
s = "x".freeze
begin; s << "y"; rescue FrozenError => e; p e.receiver; end
begin; raise "x"; rescue => e; (p e.key) rescue p $!.class; end
p (SystemExit.new(2).class rescue $!.class)
se = SystemExit.new(2)
p se.message
p se.status
p se.success?
p SystemExit.new.success?
p SystemExit.new(true, "bye").message
p SystemExit.superclass
r3 = (begin; exit(false); "no"; rescue SystemExit; "caught"; end)
p r3
r4 = (begin; exit(3); "no"; rescue SystemExit => sx; sx.status; end)
p r4
