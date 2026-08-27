# Ruby labels an aliased method's backtrace frame with the name the body was
# BORN under, while `__callee__` answers the name it was CALLED by. Both
# facts are live at once:
#
#   alias nick real; then calling `nick`
#     backtrace  -> "Al#real"      (the origin)
#     __callee__ -> :nick          (the call)
#     __method__ -> :real          (the origin)
#
# zeo carries ONE name per frame. `clif::body::BodyFnSpec` says so in its
# own doc: `origin_name` feeds `__method__`, and the frame LABEL feeds
# `__callee__`. So the label has to be the called name, and the backtrace
# reports `Al#nick` where ruby reports `Al#real`.
#
# Measured while looking for emitted-body duplication: the alias's body is a
# clone of its source's and the two emitted copies differ ONLY in this
# string, so a frame carrying both names would also let 1,098 alias sites
# across the vendored gems share one body instead of two. The fix is the
# `(origin, callee)` pair on the cold `METHOD_FRAMES` stack -- the pair must
# be CARRIED rather than swapped, because `super` inside an aliased body
# searches the original name.

class Al
  def real(x) = [__method__, __callee__, x]
  alias_method :nick, :real
  alias nick2 real
  def boom = raise("x")
  alias_method :kaboom, :boom
end

p Al.new.real(1)
p Al.new.nick(2)
p Al.new.nick2(3)
p [Al.instance_method(:nick).original_name, Al.new.method(:nick).name]

begin
  Al.new.kaboom
rescue RuntimeError => e
  puts e.backtrace.first
end

module Mixin
  def base = caller(0).first[/in '(.*)'/, 1]
  alias_method :aliased, :base
end
class User
  include Mixin
end
p [User.new.base, User.new.aliased]
