# A `return` written in a run-time `eval` returns from the METHOD the eval
# sits in, exactly as one written there would. The signal is unmarked, so it
# belongs to the nearest catcher -- and the enclosing method only pushes a
# home when the compiler can see it might be needed, which is why an `eval`
# anywhere in a body arms that catch.

def straight
  eval("return 9".dup)
  1
end
p straight

def after
  x = eval("return 8; 5".dup)
  p ["never", x]
  2
end
p after

def through_a_block
  [1, 2, 3].each do |i|
    eval("return i * 10".dup) if i == 2
  end
  :fell_through
end
p through_a_block

def under_an_ensure
  begin
    eval("return 7".dup)
  ensure
    puts "ensure ran"
  end
  :not_here
end
p under_an_ensure

def value_of_the_eval
  eval("6".dup)
end
p value_of_the_eval

# The program keeps running afterwards.
p [1, 2].map { |i| i + 1 }
