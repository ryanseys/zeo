a = ("a".."e")
p a.class
p a.to_s
p a.inspect
p a.begin
p a.end
p a.first
p a.last
p a.exclude_end?
p a.to_a
p a.size
p a.include?("c")
p a.include?("z")
p a === "c"
p a.cover?(5)
p a.map { |s| s.upcase }
p a.select { |s| s < "c" }
p a.count { |s| s < "c" }
p a.first(2)
p a == ("a".."e")
p a == ("a"..."e")
b = ("a"..."d")
p b.to_a
p b.inspect
p ("a".."e").to_s
p [*"a".."c"]
