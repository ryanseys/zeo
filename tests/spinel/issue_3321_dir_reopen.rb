class Dir
  VERSION = '1.0.0'
  def self.dot?(name)
    name == "." || name == ".."
  end
end
puts Dir::VERSION
p Dir.dot?(".")
p Dir.dot?("x")
