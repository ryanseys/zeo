# `Refinement#import_methods` copies a module's OWN method entries into the
# refinement. The runtime did the copying and reported them in
# `instance_methods(false)`, but two halves were missing: a String receiver has
# no `RObj` for the copied `MethodImpl` to bind against, and the COMPILER never
# learned the names -- so no call site a `using` covers was ever nominated and
# the imported method was unreachable however well it was copied.

module Helpers
  def helper = :helped
  def shout(s) = s.upcase
  private def secret = :secret
end

module R
  refine String do
    import_methods Helpers
    def other = :other
  end
end

module Q
  refine Integer do
    import_methods Helpers
  end
end

using R
using Q
p "x".helper
p "x".other
p "ab".shout("hi")
p 5.helper
p "x".respond_to?(:helper)

# An unrefined class is untouched.
p [].respond_to?(:helper)
begin
  [].helper
rescue NoMethodError => e
  p e.class
end

# Visibility rides along.
begin
  "x".secret
rescue NoMethodError => e
  p e.class
end
p "x".send(:secret)

# The module itself is unchanged.
p Helpers.instance_methods(false).sort

# A non-module argument is a TypeError, checked before anything imports.
module Bad
  refine String do
    begin
      import_methods 42
    rescue TypeError => e
      p e.class
    end
  end
end
__END__
:helped
:other
"HI"
:helped
true
false
NoMethodError
NoMethodError
:secret
[:helper, :shout]
TypeError
