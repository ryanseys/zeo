class A
  def tag = "a"
end
class B
  def tag = "b"
end
pick = true
x = pick ? A.new : B.new
puts x.send(:tag)
__END__
a
