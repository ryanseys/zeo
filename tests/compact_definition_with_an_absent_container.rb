# `class A::B` evaluates `A` when the definition RUNS. If nothing defines it,
# ruby raises NameError right there and the class never comes into being --
# it is not a load-time error, and the rest of the file is unaffected until
# execution reaches it. zeo used to fail the whole compile instead, which cost
# 68 gems: mostly Rails extensions reopening a host they don't depend on.

# Reached and rescued: the constant read is what raises.
begin
  class ActiveRecord::Associations::CollectionProxy
    def bulk_import(*args) = :never
  end
rescue NameError => e
  puts e.class
  puts e.message
end

# A deeper path names its OUTERMOST missing segment, not the leaf.
begin
  module Nope::Deeper::Still
  end
rescue NameError => e
  puts e.message
end

# The definition really did not happen.
p defined?(ActiveRecord)
p Object.const_defined?(:Nope)

# A container that DOES exist still works, including one reached through the
# enclosing lexical scope.
module Bundler
  module CLI; end
end
module Bundler
  class CLI::Common
    def helpful = :yes
  end
end
p Bundler::CLI::Common.new.helpful
p Bundler::CLI::Common.name

# ... and a top-anchored one.
module Outer; end
class ::Outer::Inner
  def here = :top
end
p Outer::Inner.new.here
