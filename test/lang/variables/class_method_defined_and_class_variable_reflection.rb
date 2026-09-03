class Base
  @@shared = 1
  def inherited_m; end
end
class Sub < Base
  def own_m; end
end
p Sub.method_defined?(:own_m)
p Sub.method_defined?(:inherited_m)
p Sub.method_defined?(:frozen?)
p Sub.method_defined?(:nope)
p Base.class_variable_defined?(:@@shared)
p Base.class_variable_get(:@@shared)
Base.class_variable_set(:@@shared, 42)
p Base.class_variable_get(:@@shared)
__END__
true
true
true
false
true
1
42
