# Struct#to_h builds a symbol-keyed hash of member name => value.
# Checked by lookup + length (inspect format varies across Ruby).
Person = Struct.new(:name, :age, :city)
h = Person.new("Alice", 30, "NYC").to_h
p h[:name]
p h[:age]
p h[:city]
p h.length
