# A `def self.x` on an exception subclass emits into a `pub mod` (no struct to
# attach an `impl` to), and a bare `new` inside it constructs via the runtime
# (D3) -- `Self.new` there yields the native `RubyException`, not a `new_handle`.

class D < StandardError
  def self.build(n)
    new("built-#{n}")
  end
end
e = D.build(3)
puts e.message
puts e.class
puts e.is_a?(StandardError)
__END__
built-3
D
true
