# KEYWORDS to a Struct constructor build the wrong instance. A Struct
# declared WITHOUT `keyword_init:` still accepts keywords -- ruby decides by
# whether the CALLER passed keywords, not by how the class was declared --
# and zeo takes the trailing Hash as the first member's value instead:
#
#     Pair.new(a: 1)   ruby #<struct Pair a=1, b=nil>
#                      zeo  #<struct Pair a={a: 1}, b=nil>
#
# `bind_members` decides from the argument shape plus the declared
# `keyword_init`, because the keyword-vs-positional-Hash flag ruby reads
# (`rb_keyword_given_p`) does not reach a builtin row. A real positional Hash
# must keep working, which is why the shape alone cannot settle it; the flag
# has to travel. Same root cause as
# `test/lang/methods/keyword_given_flag_does_not_travel.rb`.
#
# `keyword_init: true` is unaffected (it decides from the declaration). The
# arity-message half of the old combined gap lives in
# `struct_size_differs_on_the_compiled_path.rb`.

Pair = Struct.new(:a, :b)

p Pair.new(a: 1)
p Pair.new(a: 1, b: 2)
p Pair.new({ a: 1 })

kw = Struct.new(:a, :b, keyword_init: true)
p kw.new(a: 1)
begin
  kw.new(1)
rescue ArgumentError => e
  puts "#{e.class}: #{e.message}"
end
__END__
#<struct Pair a=1, b=nil>
#<struct Pair a=1, b=2>
#<struct Pair a={a: 1}, b=nil>
#<struct a=1, b=nil>
ArgumentError: wrong number of arguments (given 1, expected 0)
