User = Data.define(:name)
h = { user: User.new("Alice") }
case h
in { user: User[name:] }
  p name
end
u = User.new("Alice")
case u
in User[name:]
  p name          # => "Alice" under both
end
