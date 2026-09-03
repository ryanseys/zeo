# A singleton class's superclass is the singleton class of the superclass:
#
#     String.singleton_class.superclass  #=> #<Class:Object>
#
# zeo answers `Class`, collapsing the parallel hierarchy to its base. That
# chain is what makes class methods inherit -- `Sub.some_class_method` resolves
# by walking `#<Class:Sub>` -> `#<Class:Base>` -> ... -> `Class` -- so a flat
# answer describes an inheritance that is not the one actually used.
#
# Class-method inheritance itself already WORKS in zeo (the `Sub.f` line
# below), so the walk is right and only its reflection is wrong. Anything that
# reasons about the chain rather than calling through it -- a library deciding
# where to install a class method, `ancestors` on a singleton class -- reads
# the wrong shape.

p String.singleton_class.superclass
p Object.singleton_class.superclass
p BasicObject.singleton_class.superclass

class Base; def self.f = :base_f; end
class Sub < Base; end
p Sub.f
p Sub.singleton_class.superclass
p Sub.singleton_class.ancestors.first(3)

o = Object.new
p o.singleton_class.superclass
__END__
#<Class:Object>
#<Class:BasicObject>
Class
:base_f
#<Class:Base>
[#<Class:Sub>, #<Class:Base>, #<Class:Object>]
Object
