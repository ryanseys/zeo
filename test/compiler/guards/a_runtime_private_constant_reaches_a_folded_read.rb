# `private_constant` is a run-time flag: a snippet can set it on a constant
# the compiler already resolved, so a program that can `eval` at all asks
# the run time at every explicit-scope read instead of folding one.

module N1
  A = 1
  class K; end
  def self.inside = A
end

eval(["module N1; private_constant :A, :K; end", "nil"].first)

begin
  p N1::A
rescue NameError => e
  puts e.message
end
begin
  p N1::K
rescue NameError => e
  puts e.message
end

# `defined?` answers the same question, so it stands down from the fold too.
p defined?(N1::A)
p defined?(N1::K)
p N1.constants
# A bare name inside the owner's own body still reads it, and `const_get`
# ignores privacy exactly as CRuby's does.
p N1.inside
p N1.const_get(:A)

# The flag is restorable, and the read follows it.
eval(["module N1; public_constant :A; end", "nil"].first)
p N1::A
p defined?(N1::A)
__END__
private constant N1::A referenced
private constant N1::K referenced
nil
nil
[]
1
1
1
"constant"
