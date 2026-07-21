# Kernel#__id__ is an alias of #object_id.
p(42.__id__ == 42.object_id)
p("x".__id__.class)
p(nil.__id__)
p(:sym.__id__ == :sym.object_id)
