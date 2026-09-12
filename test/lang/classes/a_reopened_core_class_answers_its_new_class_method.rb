# A `def self.x` written on a core class answers like any other class method,
# including on the classes ruby grows more rows for at a require.
class Dir
  def self.label = "dir"
end

module ObjectSpace
  def self.label = "objectspace"
end

class IO
  def self.label = "io"
end

p Dir.label, ObjectSpace.label, IO.label
p Dir.send(:label)
p IO.singleton_methods(false).include?(:label)
p IO.method(:label).owner.to_s
__END__
"dir"
"objectspace"
"io"
"dir"
true
"#<Class:IO>"
