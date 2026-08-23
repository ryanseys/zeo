# An `autoload` target no longer loads BEFORE the statement holding it:
# differ.rb calls a singleton method this module body installs a few
# statements earlier (rspec-support's exact shape) -- the old eager splice
# hoisted the target's body ahead of the whole `module` statement, so the
# helper did not exist yet and the load raised NoMethodError.
#
# The target now runs at the constant's first ACCESS, as CRuby's does --
# see `autoload_is_lazy.rb`. The divergence this header used to record is
# gone; the shape is kept because it is the one the eager splice broke.
module Outer
  module Foo
    puts "body-1"
    def self.tag(x) = "tagged #{x}"
    autoload :Differ, "#{__dir__}/autoload_lazy_at_declaration/differ"
    puts "body-2"
  end
end
puts Outer::Foo::Differ.origin
