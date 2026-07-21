# structaset benchmark (from yjit-bench)
TheClass = Struct.new(:v0, :v1, :v2, :levar)

def set_value_loop(obj)
  i = 0
  while i < 1000000
    obj.levar = i
    obj.levar = i
    obj.levar = i
    obj.levar = i
    obj.levar = i
    obj.levar = i
    obj.levar = i
    obj.levar = i
    obj.levar = i
    obj.levar = i
    i = i + 1
  end
  obj.levar
end

obj = TheClass.new(1, 2, 3, 1)
puts set_value_loop(obj)
puts "done"
