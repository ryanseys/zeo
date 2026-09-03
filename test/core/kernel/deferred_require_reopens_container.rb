# A deferred (in-method) require whose file reopens a compact-path class
# (`class Outer::Feature::Policy`) BEFORE the forward-declaration it depends on
# (`module Outer::Feature`) registers -- the rubygems `Gem::Security` shape.
# The loader hoists the in-method require ahead of this file's own top-level
# statements, so analyze must resolve the container forward.
module Outer
end

# forward-declaration at load time, like rubygems/security_option.rb
module Outer::Feature
  class Policy
  end
end

def activate
  require_relative "deferred_require_reopens_container/lib"
end
activate

puts Outer::Feature::Policy.new.name
puts Outer::Feature.digest
__END__
policy
sha256
