# Test pattern matching (case/in)

# Type-based matching
def describe(val)
  case val
  in Integer
    "integer: #{val}"
  in String
    "string: #{val}"
  in Float
    "float"
  in true | false
    "boolean"
  in nil
    "nil"
  end
end

puts describe(42)       # integer: 42
puts describe("hello")  # string: hello
puts describe(3.14)     # float
puts describe(true)     # boolean
puts describe(nil)      # nil

# Value matching
def check(x)
  case x
  in 0
    "zero"
  in 1
    "one"
  in Integer
    "other int"
  end
end

puts check(0)   # zero
puts check(1)   # one
puts check(99)  # other int

puts "done"
