# `Object#initialize` takes none, so passing any is an ArgumentError --
# it was silently DROPPING them and constructing happily. Rescuable and
# raised at runtime (plan G1), covering both the static `.new` path and
# the dynamic one through a class-valued variable.

class Bare; end
begin
  Bare.new(1, 2)
rescue ArgumentError => e
  puts "ArgumentError: #{e.message}"
end
k = Bare
begin
  k.new(9)
rescue ArgumentError => e
  puts "dyn: #{e.message}"
end
p Bare.new.class
__END__
ArgumentError: wrong number of arguments (given 2, expected 0)
dyn: wrong number of arguments (given 1, expected 0)
Bare
