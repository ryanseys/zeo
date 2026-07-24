class A
end

class B < A
end

class C
  def foo
    1
  end
end

puts A.superclass
puts B.superclass
puts C.superclass
