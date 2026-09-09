# Both are reachable, and Dir's own methods still answer.
# (spinel issue #3321)
class Dir
  VERSION = '1.0.0'
  def self.dot?(name)
    name == "." || name == ".."
  end
end
puts Dir::VERSION
p Dir.dot?(".")
p Dir.dot?("x")
__END__
1.0.0
true
false
