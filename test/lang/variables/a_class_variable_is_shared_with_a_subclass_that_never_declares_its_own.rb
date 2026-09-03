class Base
  @@count = 0
  def bump
    @@count += 1
  end
  def count
    @@count
  end
end
class Sub < Base
  def bump_twice
    @@count += 1
    @@count += 1
  end
end
b = Base.new
s = Sub.new
b.bump
s.bump_twice
puts b.count
puts s.count
__END__
3
3
