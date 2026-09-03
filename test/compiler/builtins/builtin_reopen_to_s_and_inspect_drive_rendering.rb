# `to_s`/`inspect` overrides on a builtin drive `puts`, interpolation,
# `p`, CONTAINER inspect (real Ruby's rb_inspect dispatches per element --
# oracle-verified `[5].inspect` -> `[I]`), and explicit `.to_s`.

class Integer
  def to_s
    "int"
  end

  def inspect
    "I"
  end
end

puts 5
puts "v=#{5}"
p 7
puts [5].inspect
puts 6.to_s
__END__
int
v=int
I
[I]
int
