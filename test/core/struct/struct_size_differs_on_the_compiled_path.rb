# TOO MANY POSITIONALS to a Struct constructor say `struct size differs` on
# BOTH paths: the compiled constant-assigned form (which once reported a
# generic arity range -- fixed) and the runtime-minted form. The keyword half
# lives in `struct_keywords_by_call_shape.rb`.

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
__END__
ArgumentError: struct size differs
ArgumentError: struct size differs
[#<struct Pair a=1, b=nil>, #<struct Pair a=nil, b=nil>, #<struct Pair a=1, b=2>]
[1, 2]
