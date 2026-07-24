A = Struct.new(:x)
B = Struct.new(:y)
def ev(node)
  case node
  in A[x] then "a:#{x}"
  in B[y] then "b:#{y}"
  end
end
p ev(A.new(5))
p ev(B.new(9))

C = Struct.new(:p, :q)
def ev2(n)
  case n
  in A[x] then "A #{x}"
  in C[p, q] then "C #{p} #{q}"
  else "none"
  end
end
p ev2(A.new(1))
p ev2(C.new(2, 3))
p ev2(B.new(4))
