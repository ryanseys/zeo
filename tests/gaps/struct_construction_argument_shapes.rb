# Two ways a plain `Struct`'s constructor takes arguments differently from
# ruby's. Both are about `Struct` alone -- `Data`'s constructor agrees (see
# `tests/data_positional_args_are_keywords.rb`).
#
# 1. TOO MANY POSITIONALS report the wrong error. Ruby says `struct size
#    differs`; zeo reports an arity error naming a range:
#
#        Pair.new(1, 2, 3)   ruby "struct size differs"
#                            zeo  "wrong number of arguments (given 3, expected 0..2)"
#
#    Only for a Struct assigned to a CONSTANT, which the compiler recognizes
#    and materializes as a generated class -- so `.new` goes through codegen's
#    arity check rather than through `rstruct`'s `bind_members`, which does say
#    `struct size differs`. The runtime-minted form at the end already agrees,
#    which is what makes the compiled path the divergent one.
#
# 2. KEYWORDS build the wrong instance. A Struct declared WITHOUT
#    `keyword_init:` still accepts keywords -- ruby decides by whether the
#    caller passed any, not by how the class was declared -- and zeo takes the
#    trailing Hash as the first member's value instead:
#
#        Pair.new(a: 1)   ruby #<struct Pair a=1, b=nil>
#                         zeo  #<struct Pair a={a: 1}, b=nil>
#
#    `bind_members` decides from the ARGUMENT shape (a lone trailing Hash) plus
#    the declared `keyword_init`, because the keyword-vs-positional-Hash flag
#    ruby reads (`rb_keyword_given_p`) does not reach a builtin row here. A real
#    positional Hash must keep working, which is why the shape alone cannot
#    settle it; the flag has to travel. See
#    `tests/gaps/keyword_given_flag_does_not_travel.rb`, which isolates that
#    flag on `**nil` -- the same root cause with nothing else in the way.
#
# `keyword_init: true` is unaffected (it decides from the declaration), and so
# is every arity shape ruby accepts.

Pair = Struct.new(:a, :b)

begin
  Pair.new(1, 2, 3)
rescue ArgumentError => e
  puts "#{e.class}: #{e.message}"
end

p Pair.new(a: 1)
p Pair.new(a: 1, b: 2)

# A runtime-minted Struct reaches the other constructor and already agrees on
# the arity message.
anon = Struct.new(:a, :b)
begin
  anon.new(1, 2, 3)
rescue ArgumentError => e
  puts "#{e.class}: #{e.message}"
end

# These already agree and must not move.
p [Pair.new(1), Pair.new, Pair.new(1, 2)]
kw = Struct.new(:a, :b, keyword_init: true)
p kw.new(a: 1)
begin
  kw.new(1)
rescue ArgumentError => e
  puts "#{e.class}: #{e.message}"
end
p Pair.new(1, 2).to_a
