# Reflection only: dispatch, respond_to? and the methods themselves already
# agree, which runtime_extend_binds_the_class covers.
module Store
  def tag = "tagged"
end

class Klass
end
Klass.extend Store

p Klass.tag
p Klass.singleton_class.include?(Store)
p Klass.singleton_class.ancestors.include?(Store)
__END__
"tagged"
true
true
