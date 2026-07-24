# A `C.prepend(M)` CALL (explicit receiver, not the class-body `prepend M` form)
# is a static structural fact about the class ancestry for a whole-program AOT
# target -- recognized at analyze time and recorded as the same compile-time
# `prepends` edit the class-body form makes, so dispatch and `super` see the
# prepended module through the ordinary flattened MRO. No runtime metaprogramming.

module Wrap
  def greet
    "[" + super + "]"
  end
end

class Bar
  def greet
    "hi"
  end
end
Bar.prepend(Wrap)

puts Bar.ancestors.inspect
puts Bar.new.greet

# A prepend that fully overrides (no `super`).
module Loud
  def shout
    "LOUD"
  end
end
class Speaker
  def shout
    "quiet"
  end
end
Speaker.prepend(Loud)
puts Speaker.new.shout

# A qualified module name, and a prepend onto a class that already has a
# subclass -- the subclass sees the prepend through its inherited ancestry.
module Outer
  module Trace
    def run
      "trace:" + super
    end
  end
end
class Job
  def run
    "job"
  end
end
class SubJob < Job
end
Job.prepend(Outer::Trace)
puts SubJob.new.run
puts SubJob.ancestors.first(4).inspect
