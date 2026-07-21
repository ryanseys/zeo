# `puts` on a value-type sp_Class (from `obj.class`) should render
# the class name, not `printf("%lld", (long long)struct_value)`.
class Counter
  def initialize; @count = 0; end
  def increment; self; end
end

c = Counter.new
puts c.increment.class
puts Counter
