# Blocks through a builtin reopen: `yield` + an optional parameter, and an
# escaping block that captures both a mutated local (a Captured cell) and
# `self` (the `__self: RubyValue` clone path).

class Integer
  def repeat(sep = "-")
    out = ""
    i = 0
    while i < self
      out = out + yield(i).to_s
      out = out + sep if i < self - 1
      i = i + 1
    end
    out
  end

  def add_each(arr)
    total = 0
    arr.each do |x|
      total = total + x + self
    end
    total
  end
end

puts 3.repeat { |i| i * 2 }
puts 2.repeat("+") { |i| i + 1 }
puts 10.add_each([1, 2, 3])
__END__
0-2-4
1+2
36
