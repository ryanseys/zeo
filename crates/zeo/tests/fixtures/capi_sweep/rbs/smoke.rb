require "rbs"

puts RBS::Parser.parse_type("Array[Integer]").to_s
puts RBS::Parser.parse_method_type("(Integer, ?String) -> bool").to_s
_, _, decls = RBS::Parser.parse_signature("class Foo\n  def bar: (Integer) -> String\nend\n")
puts decls.first.name, decls.first.members.first.name
