# Hash#fetch raises KeyError whose #key is the key that missed, for a Symbol and an Integer.
# (spinel issue #3028)
begin
  {}.fetch(:sym)
rescue KeyError => e
  p e.key
end
begin
  {}.fetch(42)
rescue KeyError => e
  p e.key
end
begin
  {}.fetch("str")
rescue KeyError => e
  p e.key
end
p({}.fetch(:s, "d"))
p({}.fetch(7, 0))
p({}.key?(:x))
p({}["str"])
__END__
:sym
42
"str"
"d"
0
false
nil
