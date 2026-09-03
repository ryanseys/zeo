# A receiver can DIVERGE -- `raise("x").foo`, or a `const_get` whose literal
# name folds at compile time to a NameError. Their emitted type is Rust's
# never type, which coerces to anything EXCEPT behind a reference, and a
# dynamic send borrows its receiver. So the whole program failed to compile
# over an expression that never runs.
begin
  Object.const_get("Bad.Name")
rescue NameError => e
  p [e.class, e.message]
end

begin
  Object.const_get("Bad Name").to_s
rescue NameError => e
  p [e.class, e.message, e.name.to_sym, e.receiver.to_s]
end

begin
  p Object.const_get("bad").anything
rescue NameError => e
  p e.message
end

begin
  raise("boom").foo
rescue RuntimeError => e
  p [e.class, e.message]
end

begin
  p [Object.const_get("1A")]
rescue NameError => e
  p e.message
end

# The value positions that already worked, kept honest.
begin
  p({ a: (raise "in a hash") })
rescue RuntimeError => e
  p e.message
end

begin
  x = Object.const_get("also bad")
  p x
rescue NameError => e
  p e.message
end

# A well-formed name still folds to the real answer.
p Object.const_get("String")
p Object.const_get(:Integer).name
__END__
[NameError, "wrong constant name Bad.Name"]
[NameError, "wrong constant name Bad Name", :"Bad Name", "Object"]
"wrong constant name bad"
[RuntimeError, "boom"]
"wrong constant name 1A"
"in a hash"
"wrong constant name also bad"
String
"Integer"
