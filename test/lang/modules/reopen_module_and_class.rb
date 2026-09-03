# Reopening `Module` and `Class` themselves. Zeo rejected both outright --
# they were the last two builtins excluded from the reopen path, on the
# reasoning that a `RubyValue::Class` receiver has no per-value dispatch to
# hang a method on. It does: a class value's own ancestry runs `Class ->
# Module -> Object`, which is exactly the chain the MRO walk already takes, so
# the added methods register as value methods like any other builtin reopen.
#
# minitest's spec.rb opens with `class Module; def infect_an_assertion ...`
# and calls it 34 times to build the expectations DSL.
class Module
  def zeo_tag = "mod:#{name}"

  def zeo_define(sym)
    class_eval "def #{sym}; :#{sym}_ran; end"
  end
end

class Class
  def zeo_kind = "class:#{name}"
end

module M; end
class C; end
class D < C; end

# Every module and every class answers the Module reopen...
p M.zeo_tag
p C.zeo_tag
p String.zeo_tag
p Comparable.zeo_tag

# ...and only classes answer the Class reopen.
p C.zeo_kind
p D.zeo_kind
p Integer.zeo_kind
p M.respond_to?(:zeo_kind)

# A method added to Module is a real instance method of it, so the reflection
# surface agrees.
p Module.instance_method(:zeo_tag).owner
p Class.method_defined?(:zeo_tag)
p M.is_a?(Module)

# The reopened method doing what minitest's does: defining methods on the
# receiver from a computed string.
C.zeo_define(:greet)
p C.new.greet

# A reopen must not displace the builtin surface it sits beside.
p C.name
p D.superclass
p C.instance_methods(false).sort
__END__
"mod:M"
"mod:C"
"mod:String"
"mod:Comparable"
"class:C"
"class:D"
"class:Integer"
false
Module
true
true
:greet_ran
"C"
C
[:greet]
