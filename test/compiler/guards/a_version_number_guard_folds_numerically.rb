# A version gate written against the NUMBER folds at compile time even when
# the kinds mix: `RUBY_VERSION.to_f >= 2.4` (rampi), `RUBY_VERSION.to_i <
# 3.0` (sanity-ruby). The refinement activations gems guard this way NEED
# the fold -- a `using` cannot defer to runtime, it activates lexically.
module Doubling
  refine Integer do
    def doubled = self * 2
  end
end

using Doubling if RUBY_VERSION.to_f >= 2.4
p 21.doubled

# The false side: the guard folds away, so the refinement never activates.
module Halving
  refine Integer do
    def halved = self / 2
  end
end

using Halving if RUBY_VERSION.to_i < 3.0
begin
  p 42.halved
rescue NoMethodError => e
  puts e.class
end

# The same folds decide plain definition gates.
if RUBY_VERSION.to_f >= 2.4
  def modern? = true
else
  def modern? = false
end
p modern?
puts "still running"
__END__
42
NoMethodError
true
still running
