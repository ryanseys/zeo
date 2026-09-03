begin
  Object.const_get(:Nope)
rescue NameError => e
  p e.name
  p e.message
  p e.receiver
end
begin
  Nonexistent
rescue NameError => e
  p e.name
end
__END__
:Nope
"uninitialized constant Nope"
Object
:Nonexistent
