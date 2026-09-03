# A batch of builtin class/instance methods: the `Hash[]` constructor,
# `Array`/`Hash.try_convert`, `Complex.polar`, `Module#include?`, and
# `Regexp.last_match`.

# Hash[] -- the class constructor (distinct from the instance Hash#[]).
p Hash[]
p Hash[1, 2, 3, 4]
p Hash[[[:a, 1], [:b, 2]]]
p Hash[{ x: 1, y: 2 }]

# try_convert -- the arg if already the type, its to_ary/to_hash, else nil.
p Array.try_convert([1, 2, 3])
p Array.try_convert("not an array")
p Hash.try_convert({ a: 1 })
p Hash.try_convert(5)

# Complex.polar(abs, arg) -- the polar constructor.
p Complex.polar(2, 0)

# Module#include? -- true only for an included MODULE in the ancestry.
module Greetable; end
class Person
  include Greetable
end
p Person.include?(Greetable)
p Person.include?(Comparable)
begin
  Person.include?(Object)
rescue TypeError => e
  puts e.message
end

# Regexp.last_match -- the thread-local $~ and its capture groups.
"abc123" =~ /([a-z]+)(\d+)/
p Regexp.last_match(1)
p Regexp.last_match(2)
__END__
{}
{1 => 2, 3 => 4}
{a: 1, b: 2}
{x: 1, y: 2}
[1, 2, 3]
nil
{a: 1}
nil
(2+0.0i)
true
false
wrong argument type Class (expected Module)
"abc"
"123"
