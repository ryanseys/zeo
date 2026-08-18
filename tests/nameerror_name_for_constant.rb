module CM
  A = 1
end
begin
  CM.const_get(:nope)
rescue NameError => e
  p e.name
end
begin
  CM::Nope
rescue NameError => e
  p e.name
end
