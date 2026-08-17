# An alias captures the definition in effect where it appears. Aliases were a
# name mapping resolved at the end, so when the target was redefined later in
# the same body the alias answered the NEW definition.
class A
  def greet; "A"; end
  alias_method :old_greet, :greet
  def greet; "B"; end
end
p A.new.greet
p A.new.old_greet

class B
  def hi; "one"; end
  alias hi2 hi
  def hi; "two"; end
end
p B.new.hi
p B.new.hi2

# an alias with no later redefinition still maps names
class C
  def val; 7; end
  alias v val
end
p C.new.v
p C.new.val

# a wrapper that calls the captured original
class D
  def run; "base"; end
  alias_method :run_without, :run
  def run; "wrapped(" + run_without + ")"; end
end
p D.new.run
p D.new.run_without
