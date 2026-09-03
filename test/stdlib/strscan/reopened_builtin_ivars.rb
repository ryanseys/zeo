# An `@ivar` in a method of a reopened builtin.
#
# A reopened builtin has no generated struct, so zeo rejected this outright.
# It never needed one: `emit_builtin_method_fn` already marks these bodies'
# self dynamic, and `ivar_get_dyn`/`ivar_set_dyn` pick the storage tier from
# the receiver. All three tiers appear below.
#
# The gems: strscan's own truffleruby shim keeps `@string`/`@pos` on
# StringScanner, google-apis-generator and importmap-rails keep `@parent_name`
# on Class, ffi keeps `@fixed` on VariadicInvoker, socksify `@socks_peer` on
# TCPSocket.

require "strscan"

# An Object-payload builtin: the `RObj`'s own name-keyed map.
class StringScanner
  def tag=(v)
    @tag = v
  end

  def tag
    @tag
  end
end

sc = StringScanner.new("hello world")
sc.tag = :first
p sc.tag
# The native behaviour is untouched.
p sc.scan(/hello/)
p sc.rest

p StringScanner.new("x").tag

# Reflection reaches the same storage.
p sc.instance_variables
p sc.instance_variable_get(:@tag)
sc.instance_variable_set(:@tag, :second)
p sc.tag
p sc.remove_instance_variable(:@tag)
p sc.instance_variables

# A CLASS object: `civars`, keyed `(class_id, name)`.
class Class
  def parent_name=(v)
    @parent_name = v
  end

  def parent_name
    @parent_name
  end
end

class Widget; end
class Gadget; end

Widget.parent_name = "Assembly"
p Widget.parent_name
p Gadget.parent_name

# A bare heap value: `value_ivars`' identity-keyed side table.
class Array
  def label=(v)
    @label = v
  end

  def label
    @label
  end
end

xs = [1, 2, 3]
ys = [1, 2, 3]
xs.label = "counted"
p xs.label
# Identity, not equality -- `ys == xs` yet carries nothing.
p ys.label
p xs

class String
  def note=(v)
    @note = v
  end

  def note
    @note
  end
end

s = +"hi"
s.note = 9
p s.note
p s.upcase

# Reaching the same storage through a method defined on the reopened builtin
# rather than at the call site, so the read and the write cross a call boundary.
class Array
  def bump
    @label = "#{@label}!"
    self
  end
end

p xs.bump.label
__END__
:first
"hello"
" world"
nil
[:@tag]
:first
:second
:second
[]
"Assembly"
nil
"counted"
nil
[1, 2, 3]
9
"HI"
"counted!"
