class Base
  @@count = 0
  def count
    @@count
  end
end
class Other
  @@count = 100
  def count
    @@count
  end
end
puts Base.new.count
puts Other.new.count
__END__
0
100
