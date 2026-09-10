# `def self.x(a, b = nil)` on a class and on a module, called with the optional
# argument omitted and supplied.

# Module form: cls method lives in @meth_* under "<Mod>_cls_<m>".
# Two gaps: the dispatch path used the un-typed compile_call_args
# instead of compile_call_args_with_defaults, AND param types
# weren't widened from `<Mod>.<m>(args)` call sites (issue #207's
# logic only walked @cls_cmeth_*).
module M
  def self.greet(name, msg = "default")
    name + "/" + msg
  end
end

puts M.greet("a")            # a/default
puts M.greet("a", "world")   # a/world

# Class form: cls method lives in @cls_cmeth_*. Defaults weren't
# stored at all (no @cls_cmeth_defaults table parallel to
# @cls_meth_defaults).
class C
  def self.greet(name, msg = "fromcls")
    name + "/" + msg
  end
end

puts C.greet("b")            # b/fromcls
puts C.greet("b", "explicit")  # b/explicit

# Inheritance + default: subclass inherits the default through
# the propagate_inherited_class_methods synthetic copy.
class Sub < C
end

puts Sub.greet("c")          # c/fromcls
__END__
a/default
a/world
b/fromcls
b/explicit
c/fromcls
