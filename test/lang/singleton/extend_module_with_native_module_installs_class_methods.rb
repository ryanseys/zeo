# `SomeModule.extend(Comparable)` -- a native module's methods become the
# receiver's own module methods, running with the module as `self`.

module Wrapper
  def self.<=>(other); 0; end
end
Wrapper.extend(Comparable)
puts Wrapper.respond_to?(:clamp)
puts Wrapper.between?(Wrapper, Wrapper)
__END__
true
true
