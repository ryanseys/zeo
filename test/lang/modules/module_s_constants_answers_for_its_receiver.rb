TOP_LEVEL_MARK = 1
module M
  IN_M = 2
end

# A bare `Module.constants` is the lexical-scope query.
p Module.constants.include?(:TOP_LEVEL_MARK)

# Given an argument it stops being that and asks Module's own set.
p Module.constants(false)

# Given ANY OTHER receiver it asks that receiver's, whatever the arity. Every
# singleton class of a module inherits this row -- `#<Class:M>.singleton_class`
# has `#<Class:Module>` as its superclass -- so this is the path
# `M.singleton_class.constants` takes.
sc = M.singleton_class
sc.const_set(:ON_THE_SINGLETON, 3)
p sc.constants(false)
p sc.constants.include?(:ON_THE_SINGLETON)
p M.constants
p M.singleton_class.const_get(:ON_THE_SINGLETON)
__END__
true
[]
[:ON_THE_SINGLETON]
true
[:IN_M]
3
