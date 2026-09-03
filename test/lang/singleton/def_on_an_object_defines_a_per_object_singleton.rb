# `def obj.name` and `class << obj` install per-object singleton
# methods (identity-keyed overlay), reaching `@ivar`/`self`/params and
# answering `respond_to?` only on that object.

obj = Object.new
obj.instance_variable_set(:@n, 10)
def obj.double
  @n * 2
end
class << obj
  def plus(k)
    @n + k
  end
end
puts obj.double
puts obj.plus(5)
puts obj.respond_to?(:double)
puts obj.respond_to?(:plus)
puts Object.new.respond_to?(:double)
__END__
20
15
true
true
false
