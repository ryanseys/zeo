# A BUILTIN row declared `protected` read as public. The DSL parsed
# `private`/`protected` in front of a `def`, but the generated method table
# carried ONE bit -- `is_private` -- so `protected` collapsed into public: the
# method answered an outside caller, `protected_instance_methods` was empty,
# and `public_instance_methods` listed it.
#
# The table now carries the third state (`is_protected`, a companion fn rather
# than a widened `is_private`, because the readers ask the two questions
# separately), and both reflection readers plus `instance_method_visibility`
# consult it. Enforcement then comes for free: the explicit-receiver barrier
# already reads that one function.
#
# `Pathname#path` is the whole population today -- ruby makes it protected so
# `#to_s` is the way to spell a path out loud.

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

# A protected builtin row IS reachable when the CALLER's self is a kind of the
# owner, which is the whole point of the third state rather than a second
# flavour of private. Spelled as a REOPEN, because zeo decides the caller from
# the class the call site sits in -- a compile-time answer, so `instance_eval`
# does not move it (see tests/gaps/protected_reach_from_instance_eval.rb).
class Pathname
  def same_path?(other) = path == other.path
end
show("kin caller") { Pathname.new("a").same_path?(Pathname.new("a")) }
show("kin caller differs") { Pathname.new("a").same_path?(Pathname.new("b")) }
show("non-kin caller") { Object.new.instance_eval { Pathname.new("b").path } }
show("send reaches it") { Pathname.new("a").send(:path) }
show("public_send does not") { Pathname.new("a").public_send(:path) }
show("respond_to?") { Pathname.new("a").respond_to?(:path) }
show("respond_to? all") { Pathname.new("a").respond_to?(:path, true) }
show("protected_method_defined?") { Pathname.protected_method_defined?(:path) }
show("private_method_defined?") { Pathname.private_method_defined?(:path) }
show("public_method_defined?") { Pathname.public_method_defined?(:path) }
