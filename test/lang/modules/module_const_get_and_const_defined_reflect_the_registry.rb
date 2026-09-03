module M
  X = 7
  module N
  end
end
puts M.const_get(:X)
puts M.const_get("X")
p M.const_defined?(:X)
p M.const_defined?(:Nope)
p M.const_defined?(:N)
p M.const_get(:N).is_a?(Module)
begin; M.const_get(:Missing); rescue NameError => e; puts e.message; end
begin; M.const_defined?("bad"); rescue NameError => e; puts e.message; end
__END__
7
7
true
false
true
true
uninitialized constant M::Missing
wrong constant name bad
