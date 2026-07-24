Rec = Struct.new(:id, :name)

full = Rec.new(1, "alice")
puts full.id
puts full.name

empty = Rec.new
puts empty.id.nil?
puts empty.name.nil?

empty.id = 7
empty.name = "bob"
puts empty.id
puts empty.name
