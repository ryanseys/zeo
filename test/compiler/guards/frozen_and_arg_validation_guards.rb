# Frozen mutators (String insert/prepend/slice!, Hash []=/merge!/clear) and
# arg validation (Integer base, Set enumerable, Random#bytes, Float#divmod
# NaN) raise the CRuby exception class rather than silently mutating.

require "set"
p("s".freeze.instance_of?(String)) # keep require used, warm path
p(("abc".freeze.insert(0, "x") rescue $!.class))
p(("abc".freeze.slice!(0) rescue $!.class))
p(({a: 1}.freeze.merge!({b: 2}) rescue $!.class))
p(({a: 1}.freeze.clear rescue $!.class))
h = {x: 5}.freeze
p((begin; h[:x] += 1; rescue => e; e.class; end))
p((Integer(5, 16) rescue $!.class))
p((Set.new(5) rescue $!.class))
p(((Set[1, 2] & 5) rescue $!.class))
p((Random.new(1).bytes(-1) rescue $!.class))
p((1.0.divmod(0.0 / 0.0) rescue $!.class))
__END__
true
FrozenError
FrozenError
FrozenError
FrozenError
FrozenError
ArgumentError
ArgumentError
ArgumentError
ArgumentError
FloatDomainError
