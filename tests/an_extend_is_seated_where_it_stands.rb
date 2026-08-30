# `rb_extend_object` is `rb_include_module(rb_singleton_class(obj), module)`,
# so a class body's `extend M` seats M in the singleton chain WHERE THE
# STATEMENT STANDS. zeo knows the edge at compile time and used to have it in
# place from program start, which made four things visible too early.

module MR
  def method_removed(n) = ((@removed ||= []) << n)
  def method_undefined(n) = ((@undefined ||= []) << n)
  def method_added(n) = ((@added ||= []) << n)
  def removed = @removed
  def undefined = @undefined
  def added = @added
end

# A hook the module supplies does not fire for a definition written above the
# `extend`.
class CR
  def a; end
  def b; end
  remove_method :a
  extend MR
  remove_method :b
end
p CR.removed

class CU
  def a; end
  def b; end
  undef_method :a
  extend MR
  undef_method :b
end
p CU.undefined

class CA
  def a; end
  extend MR
  def b; end
end
p CA.added

# The singleton chain gains the module at the statement, not before.
module MX; end
class CX
  p singleton_class.ancestors.include?(MX)
  p is_a?(MX)
  extend MX
  p singleton_class.ancestors.include?(MX)
  p is_a?(MX)
end
p CX.singleton_class.ancestors.include?(MX)

# A class method the module supplies is not callable above the `extend`.
module MC
  def helper = :from_mc
end
class CC
  begin
    helper
  rescue NoMethodError, NameError => e
    p e.class
  end
  extend MC
  p helper
end
p CC.helper

# A class with its OWN def keeps it, above the `extend` and below.
class CO
  def self.helper = :own
  p helper
  extend MC
  p helper
end

# Two modules extended by one class: the LAST one seated wins a shared name,
# which is where `rb_include_module` puts it.
module M1
  def which = :one
end
module M2
  def which = :two
end
class CW
  extend M1
  extend M2
end
p CW.which
p CW.singleton_class.ancestors.index(M2) < CW.singleton_class.ancestors.index(M1)

# The `extended` hook still fires, once, at the statement.
module MH
  def self.extended(base) = puts("extended #{base}")
end
class CH
  puts "before"
  extend MH
  puts "after"
end
