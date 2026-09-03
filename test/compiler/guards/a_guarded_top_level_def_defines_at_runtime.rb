# A guarded top-level plain `def` (myrrha's `def Boolean(s)` behind
# `Myrrha.core_ext?`): defined -- as a private Object instance method --
# exactly when the branch runs.
module Switch
  def self.on?
    ENV["GUARDED_DEF_TEST"] != "off"
  end
end

if Switch.on?
  GUARD_TOOK = true
  def taken_helper(x)
    "took #{x}"
  end
else
  GUARD_TOOK = false
  def skipped_helper(x)
    "skipped #{x}"
  end
end

p GUARD_TOOK
p taken_helper(1)
p Object.private_method_defined?(:taken_helper)
p Object.private_method_defined?(:skipped_helper)

# Callable via implicit self from inside another object's method (a private
# Object instance method, unlike def self.x on main).
class Elsewhere
  def call_it
    taken_helper(:from_method)
  end
end
p Elsewhere.new.call_it

# Explicit receiver stays forbidden.
begin
  Object.new.taken_helper(2)
rescue NoMethodError => e
  puts "explicit receiver: #{e.class}"
end

# The untaken branch's method genuinely does not exist.
begin
  skipped_helper(3)
rescue NoMethodError => e
  puts "untaken: #{e.class}"
end
__END__
true
"took 1"
true
false
"took from_method"
explicit receiver: NoMethodError
untaken: NoMethodError
