# `TOPLEVEL_BINDING` IS the main script's frame, so it reads and writes the
# same locals the file does -- and `Kernel#eval`'s own binding/filename/lineno
# arguments run their source in whichever scope they name.
zz = 12

puts "-- the constant is the top-level frame --"
p TOPLEVEL_BINDING.class
p TOPLEVEL_BINDING.source_location
p TOPLEVEL_BINDING.local_variables
p TOPLEVEL_BINDING.local_variable_get(:zz)

puts "-- Kernel#eval's binding argument --"
p eval("zz + 1", binding)
p eval("zz + 2", TOPLEVEL_BINDING)
p eval("zz + 3", nil)
begin
  eval("1", 5)
rescue TypeError => e
  p [e.class, e.message]
end

puts "-- filename and lineno --"
p eval("__FILE__", binding, "fake.rb", 10)
p eval("__LINE__", binding, "fake.rb", 10)
p eval("__LINE__\n__LINE__", binding, "fake.rb", 10)

puts "-- dup shares the frame, not the additions --"
d = TOPLEVEL_BINDING.dup
d.local_variable_set(:only_on_the_copy, 1)
p TOPLEVEL_BINDING.local_variable_defined?(:only_on_the_copy)
p d.local_variable_defined?(:only_on_the_copy)
d.local_variable_set(:zz, 99)
p zz
p TOPLEVEL_BINDING.dup.local_variable_defined?(:only_on_the_copy)
__END__
-- the constant is the top-level frame --
Binding
["<main>", 0]
[:zz, :e, :d]
12
-- Kernel#eval's binding argument --
13
14
15
[TypeError, "wrong argument type Integer (expected binding)"]
-- filename and lineno --
"fake.rb"
10
11
-- dup shares the frame, not the additions --
false
true
99
false
