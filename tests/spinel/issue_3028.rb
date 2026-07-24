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
