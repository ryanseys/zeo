# An `autoload` target no longer loads BEFORE the statement holding it:
# differ.rb calls a singleton method this module body installs a few
# statements earlier (rspec-support's exact shape) -- the old eager splice
# hoisted the target's body ahead of the whole `module` statement, so the
# helper did not exist yet and the load raised NoMethodError.
#
# KNOWN DIVERGENCE (unobserved here): zeo runs the target when the
# `autoload` DECLARATION executes; CRuby waits for the first constant
# access. The target is side-effect-free so both orders print alike.
module Outer
  module Foo
    puts "body-1"
    def self.tag(x) = "tagged #{x}"
    autoload :Differ, "#{__dir__}/autoload_lazy_at_declaration/differ"
    puts "body-2"
  end
end
puts Outer::Foo::Differ.origin
