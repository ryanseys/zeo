# The second boolean decides whether ancestors are searched, which is what a
# bare `require "uri"` needs.
module Foo
  X = 1
end
p Foo.const_defined?(:X, false)
__END__
true
