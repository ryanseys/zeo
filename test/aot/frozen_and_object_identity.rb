# Object identity and freezing across the linked runtime's allocator.
a = "mutable".dup
b = a.dup
p a == b, a.equal?(b)
a.freeze
p a.frozen?, b.frozen?
begin
  a << "!"
rescue FrozenError => e
  puts e.class
end
p 1.frozen?, :sym.frozen?, nil.frozen?
p Object.new.frozen?
__END__
true
false
true
false
FrozenError
true
true
true
false
