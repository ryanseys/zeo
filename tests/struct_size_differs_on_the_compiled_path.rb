# TOO MANY POSITIONALS to a Struct constructor report the wrong error. Ruby
# says `struct size differs`; zeo reports an arity error naming a range:
#
#     Pair.new(1, 2, 3)   ruby "struct size differs"
#                         zeo  "wrong number of arguments (given 3, expected 0..2)"
#
# Only for a Struct assigned to a CONSTANT, which the compiler recognizes and
# materializes as a generated class -- so `.new` goes through codegen's arity
# check rather than through `rstruct`'s `bind_members`, which does say
# `struct size differs`. The runtime-minted form below already agrees, which
# is what makes the compiled path the divergent one. Fix shape: the
# synthesized initialize delegates to a binder on `bind_members` instead of a
# fixed optional-positional signature.
#
# The keyword half of the old combined gap lives in
# `struct_keywords_by_call_shape.rb`.

Pair = Struct.new(:a, :b)

begin
  Pair.new(1, 2, 3)
rescue ArgumentError => e
  puts "#{e.class}: #{e.message}"
end

anon = Struct.new(:a, :b)
begin
  anon.new(1, 2, 3)
rescue ArgumentError => e
  puts "#{e.class}: #{e.message}"
end

# These already agree and must not move.
p [Pair.new(1), Pair.new, Pair.new(1, 2)]
p Pair.new(1, 2).to_a
