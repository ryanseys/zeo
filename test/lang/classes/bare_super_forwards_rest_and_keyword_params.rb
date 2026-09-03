class C
  def count(*nums)
    nums.length
  end
end

class D < C
  def count(*nums)
    super * 10
  end
end

puts D.new.count(1, 2, 3)

class E
  def kw(a:, b: 2)
    "a=#{a} b=#{b}"
  end
end

class F < E
  def kw(a:, b: 2)
    "got " + super
  end
end

puts F.new.kw(a: 1, b: 9)
__END__
30
got a=1 b=9
