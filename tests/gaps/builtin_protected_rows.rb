# A BUILTIN row declared `protected` reads as public.
#
# The DSL parses `private`/`protected` in front of a `def`, but the method
# table carries one bit -- `is_private` -- so `protected` collapses into
# public: the method answers an outside caller, `protected_instance_methods`
# is empty, and `public_instance_methods` lists it.
#
# `Pathname#path` is the whole population today: ruby makes it protected so
# that `#to_s` is the way to spell a path out loud. A method defined in RUBY
# is unaffected -- `class C; protected def m; end; end` is enforced exactly.
#
# Fix shape: widen the macro's `Entry.is_private` into the three-state
# visibility the DSL already parses, and teach the two reflection readers and
# the call-site check to read it.

def show(label)
  puts "#{label}: #{yield.inspect}"
rescue Exception => e # rubocop:disable Lint/RescueException
  puts "#{label}: #{e.class}: #{e.message}"
end

show("calling it") { Pathname.new("a").path }
show("protected_instance_methods") { Pathname.protected_instance_methods(false) }
show("public lists it") { Pathname.public_instance_methods(false).include?(:path) }
show("instance_methods lists it") { Pathname.instance_methods(false).include?(:path) }

# A Ruby-defined protected method is enforced, which is what makes this a
# builtin-table gap rather than a dispatch one.
class Probe
  protected def secret = 1
end
show("ruby-defined protected") { Probe.new.secret }
