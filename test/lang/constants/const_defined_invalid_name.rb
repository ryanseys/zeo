# Module#const_defined? validates the constant name. A defined name answers
# true, an undefined-but-well-formed name answers false, and a malformed name
# (lowercase / leading underscore / a non-identifier byte) raises NameError
# "wrong constant name <name>" rather than answering false. A literal name
# and a name computed at runtime both reject.
module M
  X = 1
end

p M.const_defined?("X")   # true
p M.const_defined?(:X)    # true
p M.const_defined?("Y")   # false (well-formed, undefined)
p M.const_defined?(:Zz)   # false
p M.const_defined?("X1")  # false

def check(&blk)
  blk.call
  puts "no raise"
rescue NameError => e
  puts e.message
end

check { M.const_defined?("name") }
check { M.const_defined?("_Foo") }
check { M.const_defined?("@x") }
check { M.const_defined?("1A") }
check { M.const_defined?("A B") }
check { M.const_defined?("") }
check { M.const_defined?(:lower) }
__END__
true
true
false
false
false
wrong constant name name
wrong constant name _Foo
wrong constant name @x
wrong constant name 1A
wrong constant name A B
wrong constant name 
wrong constant name lower
