# `Warning.warn` is the module's own instance method, reached because Warning
# extends itself.
p Warning.singleton_methods(false).sort
p Warning.instance_methods(false).sort
p Warning.method(:warn).owner.to_s
p Warning.singleton_class.ancestors.first(2).map(&:to_s)
p Warning.respond_to?(:warn), Warning.instance_method(:warn).owner.to_s
__END__
[:[], :[]=, :categories]
[:warn]
"Warning"
["#<Class:Warning>", "Warning"]
true
"Warning"
