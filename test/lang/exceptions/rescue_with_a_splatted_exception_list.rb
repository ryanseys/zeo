ERRS = [ArgumentError, TypeError]

# A constant array of classes.
begin
  raise TypeError, "t"
rescue *ERRS => e
  puts "const #{e.class}"
end

# A local array; no `=> e` capture.
errs = [KeyError]
begin
  raise KeyError, "k"
rescue *errs
  puts "local no-capture"
end

# Static class mixed with a splat, in either order.
extra = [TypeError]
begin; raise TypeError, "t"; rescue ArgumentError, *extra => e; puts "mixed1 #{e.class}"; end
lead = [KeyError]
begin; raise TypeError, "t"; rescue *lead, TypeError => e; puts "mixed2 #{e.class}"; end

# Splatting a single class (not an array) wraps it: `*one` == `[one]`.
one = RuntimeError
begin; raise "r"; rescue *one => e; puts "single #{e.class}"; end

# An empty array catches nothing -> propagates.
empty = []
begin
  begin; raise TypeError, "t"; rescue *empty; puts "wrong"; end
rescue TypeError
  puts "empty catches nothing"
end

# A non-matching splat re-raises to an outer handler.
only_arg = [ArgumentError]
begin
  begin; raise TypeError, "t"; rescue *only_arg; puts "wrong"; end
rescue TypeError => e2
  puts "propagated #{e2.class}"
end

# ensure still runs; $! is the caught exception; module-include matches.
module Alertable; end
class Alarmed < StandardError; include Alertable; end
mods = [Alertable]
begin
  raise Alarmed, "a"
rescue *mods
  puts "module-match #{$!.class}"
ensure
  puts "ensure ran"
end

# Splat rescue as a method-body clause, with retry.
E = [RuntimeError]
def flaky
  @n = (@n || 0) + 1
  raise "x" if @n < 2
  "ok n=#{@n}"
rescue *E
  retry if @n < 2
end
puts flaky
__END__
const TypeError
local no-capture
mixed1 TypeError
mixed2 TypeError
single RuntimeError
empty catches nothing
propagated TypeError
module-match Alarmed
ensure ran
ok n=2
