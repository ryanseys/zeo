# One name resolves to ONE definition, so the class that defines it decides
# which reflection list it lands in. `private_methods` walked the whole chain
# and let Kernel's private `exec` back onto the list under a class that had
# just defined a PUBLIC `exec` -- the name went unclaimed whenever the filter
# rejected it, so the next ancestor took it.
#
# thor asks exactly this before it runs a command (`instance.private_methods &
# [name]`), so `bundle exec` answered `Could not find command "exec"` for a
# command bundler defines in the ordinary way.

class Runner
  def exec(*args) = [:mine, *args]
  def load(f) = [:loaded, f]
end

r = Runner.new
p r.private_methods.include?(:exec)
p r.public_methods.include?(:exec)
p r.private_methods.include?(:load)
p r.public_methods.include?(:load)
p Runner.private_instance_methods.include?(:exec)
p Runner.public_instance_methods.include?(:exec)
p r.exec(1)

# A class that does NOT redefine one keeps Kernel's own visibility.
class Bare; end
p Bare.new.private_methods.include?(:exec)
p Bare.new.public_methods.include?(:exec)
