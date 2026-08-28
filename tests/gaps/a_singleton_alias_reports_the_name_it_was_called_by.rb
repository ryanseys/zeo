# GAP: `__callee__` inside a SINGLETON method reached through an alias answers
# the name the body was defined under, not the one it was called by.
#
# `__method__` answers the DEFINED name and `__callee__` the REACHED name, and
# an alias is the only thing that tells them apart. zeo answers the defined
# name for both here.
#
# The instance-method case is already right; this is the singleton half. Found
# while fixing `__method__`, which used to answer `:"Owner.name"` for every
# class method -- see tests/a_class_method_knows_its_own_name.rb, which pins
# everything that IS right.
#
# WHAT IT NEEDS: the runtime row reads the innermost frame's label, and a
# frame entered through an alias carries the ORIGINAL body's label. The entry
# knows the name it was found under; the frame does not carry it.
class Aliased
  def self.original = __callee__
  singleton_class.alias_method :nickname, :original
end

puts "original #{Aliased.original.inspect}"
puts "alias    #{Aliased.nickname.inspect}"
