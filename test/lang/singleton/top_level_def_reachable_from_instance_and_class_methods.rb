def helper(x)
  x + 1
end
class Widget
  def go
    helper(4)
  end
  def self.direct
    helper(10)
  end
end
puts Widget.new.go
puts Widget.direct
__END__
5
11
