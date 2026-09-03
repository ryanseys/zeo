# A bare `@x = ...` directly in a class body -- the ordinary way
# class-level state gets seeded. It used to fall through `register_class`'s
# catch-all arm and be SILENTLY DROPPED, leaving the reader a bare nil
# with no diagnostic at all.

class Registry
  @items = []
  @count = 0
  def self.add(x); @items << x; @count += 1; self; end
  def self.items; @items; end
  def self.count; @count; end
end
Registry.add("a").add("b")
p Registry.items
p Registry.count
__END__
["a", "b"]
2
