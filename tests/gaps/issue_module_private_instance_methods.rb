# A bare `private` inside a MODULE body does not mark the module's own
# instance methods private. The same directive inside a CLASS body works, so
# the visibility machinery is there -- the module's class-body scope frame is
# not feeding it.
#
# Found while goldening `Module#dup`: the copy faithfully reproduces the
# original's wrong answer, so `tests/module_copy_and_extend_primitives.rb`
# trims the private case out.
module Sample
  def a = 1

  private

  def b = 2
end

p Sample.instance_methods(false).sort
p Sample.private_instance_methods(false).sort
p Sample.private_method_defined?(:b)
