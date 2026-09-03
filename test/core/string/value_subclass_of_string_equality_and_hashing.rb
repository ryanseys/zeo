# A `String` subclass (D3): inherited `String` methods, a `super`-free custom
# method, `dup` independence, and payload-based equality/`Hash`-key identity
# (`Tag.new("k")` is the same key as `"k"`, symmetric `==`).

class Tag < String
  def shout; upcase + "!"; end
end
t = Tag.new("hi")
puts t.shout
puts t.class
puts(t == "hi")
puts("hi" == t)
puts(Tag.new("x").hash == "x".hash)
h = { "key" => 1 }
puts h[Tag.new("key")].inspect
d = Tag.new("a")
d2 = d.dup
d2 << "b"
puts d
puts d2
__END__
HI!
Tag
true
true
true
1
a
ab
