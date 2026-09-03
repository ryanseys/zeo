class Array
  def detect
    :from_the_reopen
  end

  def reduce(seed)
    :also_from_the_reopen
  end
end

arr = [1, 2, 3]

# An alias and its twin are separate method entries, so redefining either one
# takes the whole kind off the fused path.
p arr.find { |x| x > 1 }
p arr.detect { |x| x > 1 }
p arr.inject(0) { |a, b| a + b }
p arr.reduce(0) { |a, b| a + b }

# The kinds nobody touched still answer normally.
p arr.count { |x| x > 1 }
p arr.none? { |x| x > 9 }
p arr.all? { |x| x > 0 }
p arr.any? { |x| x > 2 }
__END__
2
:from_the_reopen
6
:also_from_the_reopen
2
true
true
true
