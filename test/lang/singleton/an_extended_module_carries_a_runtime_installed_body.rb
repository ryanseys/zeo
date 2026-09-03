# A `def` a module installs at RUN TIME -- one whose branch only the run time
# picks -- is reachable through `extend` exactly as a statically registered
# one is. fileutils writes the shape: `fu_windows?` is defined inside a
# `case` on the host os, and the module body then calls it on itself.

module Platform
  case (defined?(::RbConfig) ? ::RbConfig::CONFIG["host_os"] : ::RUBY_PLATFORM)
  when /mswin|mingw/
    def windows?; true end
  else
    def windows?; false end
  end

  def steady?; :steady end
end

module Host
  include Platform
  extend Platform

  # Reached through the singleton chain the `extend` opened, while the
  # module body is still running.
  ANSWER = windows?
  STEADY = steady?
end

p Host::ANSWER
p Host::STEADY
p Host.windows?
p Host.steady?

# The predicate must agree with the call: a name the send answers cannot be
# one `respond_to?` denies.
p Host.respond_to?(:windows?)
p Host.respond_to?(:steady?)
p Host.singleton_class.ancestors.include?(Platform)

# The include half serves instances, unchanged.
class Box
  include Platform
end
p Box.new.windows?
p Platform.instance_methods(false).sort
p Platform.instance_method(:windows?).owner

# `extend` on an object, not a module.
obj = Object.new
obj.extend(Platform)
p obj.windows?
p obj.respond_to?(:windows?)
__END__
false
:steady
false
:steady
true
true
true
false
[:steady?, :windows?]
Platform
false
true
