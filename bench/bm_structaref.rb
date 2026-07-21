# structaref benchmark (from yjit-bench)
TheClass = Struct.new(:v0, :v1, :v2, :levar)

def get_value_loop(obj)
  sum = 0
  i = 0
  while i < 1000000
    sum = sum + obj.levar
    sum = sum + obj.levar
    sum = sum + obj.levar
    sum = sum + obj.levar
    sum = sum + obj.levar
    sum = sum + obj.levar
    sum = sum + obj.levar
    sum = sum + obj.levar
    sum = sum + obj.levar
    sum = sum + obj.levar
    i = i + 1
  end
  sum
end

obj = TheClass.new(1, 2, 3, 1)
puts get_value_loop(obj)
puts "done"
