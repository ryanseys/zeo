# `undef :"#{name}"` (immutable_struct_ex strips Struct writers in a loop)
# has no compile-time spelling: it desugars to a runtime `undef_method` on
# the default definee, in class-body order.
class Wardrobe
  def hat; "hat"; end
  def hat=(_v); end
  def coat; "coat"; end
  def coat=(_v); end

  %w[hat coat].each do |piece|
    undef :"#{piece}="
  end
end

w = Wardrobe.new
p w.hat
p w.coat
begin
  w.hat = 1
rescue NoMethodError => e
  puts "writer gone: #{e.class}"
end
p w.respond_to?(:coat=)

# Inside instance_eval on a singleton (the immutable_struct_ex shape), and
# mixed with a plain name in one statement.
class Locker
  def a; :a; end
  def b; :b; end
  def c; :c; end
  gone = "b"
  undef :a, :"#{gone}"
end
l = Locker.new
p l.c
p l.respond_to?(:a)
p l.respond_to?(:b)
puts "still running"
__END__
"hat"
"coat"
writer gone: NoMethodError
false
:c
false
false
still running
