# A class IS a constant of the scope it sits in, and reflection has to see it
# under a name computed at run time -- an autoloader, `Marshal`, a YAML class
# tag and a plain `Object.const_get(config_string)` all resolve that way.
# zeo registers a class by qualified name and never writes it to the constant
# table (codegen resolves `Zlib::Error` statically), so both halves of
# constant reflection have to consult the registry too.

require "zlib"

puts "-- const_get with a computed name"
%w[Array String Comparable Float Struct Enumerator Math Zlib RUBY_VERSION].each do |n|
  begin
    puts "#{n} -> #{Object.const_get(n)}"
  rescue NameError
    puts "#{n} -> NameError"
  end
end

puts "-- nested, also computed"
%w[Error GzipFile].each do |n|
  puts "Zlib::#{n} -> #{Zlib.const_get(n)}"
end

puts "-- const_defined? agrees with const_get"
["Array", "Zlib", "RUBY_VERSION", "Nope"].each do |n|
  found = Object.const_defined?(n)
  resolves = begin
    Object.const_get(n)
    true
  rescue NameError
    false
  end
  puts "#{n}: defined?=#{found} get=#{resolves}"
end

puts "-- a class sees a top-level constant through its ancestry"
p String.const_get("Comparable")
p Comparable.const_get("Array")
p Object.const_get("Object")

puts "-- and misses report the receiver"
begin
  Object.const_get("Nope")
rescue NameError => e
  puts e.message
end
begin
  Zlib.const_get("Nope")
rescue NameError => e
  puts e.message
end

puts "-- listing"
p Object.constants.include?(:Array)
p Object.constants.include?(:Zlib)
p Object.constants.include?(:RUBY_VERSION)
p Object.constants.size > 100
p Zlib.constants.include?(:Error)
p Array.constants
p String.constants

module Holder
  INNER = 1
  class Nested; end
end
p Holder.constants.sort
p Holder.const_get("Nested")
p Holder.const_get("INNER")

puts "-- Module.constants is the singleton form, not Module#constants"
p Module.constants.include?(:Array)
p Module.constants.include?(:RUBY_VERSION)
p Module.constants.size > 100
__END__
-- const_get with a computed name
Array -> Array
String -> String
Comparable -> Comparable
Float -> Float
Struct -> Struct
Enumerator -> Enumerator
Math -> Math
Zlib -> Zlib
RUBY_VERSION -> 4.0.6
-- nested, also computed
Zlib::Error -> Zlib::Error
Zlib::GzipFile -> Zlib::GzipFile
-- const_defined? agrees with const_get
Array: defined?=true get=true
Zlib: defined?=true get=true
RUBY_VERSION: defined?=true get=true
Nope: defined?=false get=false
-- a class sees a top-level constant through its ancestry
Comparable
Array
Object
-- and misses report the receiver
uninitialized constant Nope
uninitialized constant Zlib::Nope
-- listing
true
true
true
true
true
[]
[]
[:INNER, :Nested]
Holder::Nested
1
-- Module.constants is the singleton form, not Module#constants
true
true
true
