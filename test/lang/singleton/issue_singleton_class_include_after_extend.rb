# `Klass.extend M` INSTALLS M's methods as class methods rather than splicing
# M into the class's singleton ancestry, so the singleton class does not report
# M among its ancestors. Reflection-only: dispatch, `respond_to?` and the
# methods' own behaviour are all correct -- see
# `tests/runtime_extend_binds_the_class.rb`. Documented under "`extend` on a
# class, at runtime" in `docs/COMPATIBILITY.md`.
#
# Found via singleton.rb's
# `Once.singleton_class.include?(Singleton::SingletonClassMethods)`.
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
