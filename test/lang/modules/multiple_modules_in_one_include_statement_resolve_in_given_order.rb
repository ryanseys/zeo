module A
  def a
    "a"
  end
end
module B
  def b
    "b"
  end
end
class C
  include A, B
end
c = C.new
puts c.a
puts c.b
__END__
a
b
