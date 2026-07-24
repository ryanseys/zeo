# `case <scalar> when ClassConst` previously compiled the
# class-constant when-arm into a strcmp against the sp_Class
# struct, failing C compile with "incompatible type for argument
# 2 of strcmp". The same shape applied to int / float / symbol /
# bool predicates would have compared the predicate's value
# against an sp_Class struct. Fix: at compile_when_conds, when
# the when-arg is a ConstantReadNode naming a built-in class,
# resolve via Module#=== — match only when the class corresponds
# to the predicate's kind (Integer / Float for numeric, String
# for string, Symbol for symbol, TrueClass / FalseClass for
# bool). Anything else is a static false.

class Box
  attr_accessor :v
  def initialize; @v = nil; end
  def setstr(s); @v = s; end
end

def classify(b)
  v = b.v
  case v
  when String
    "string"
  else
    "other"
  end
end

b1 = Box.new
b1.setstr("hello")
puts classify(b1)

# int predicate + class consts
n = 42
case n
when Integer
  puts "int-matched"
when String
  puts "wrong"
else
  puts "no"
end

# sym predicate + class consts
s = :hello
case s
when Symbol
  puts "sym-matched"
when String
  puts "wrong-sym"
else
  puts "no-sym"
end
