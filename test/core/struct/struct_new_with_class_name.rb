Struct.new("Named", :a)
p Struct::Named.new(1).a
p Struct::Named.name
__END__
1
"Struct::Named"
