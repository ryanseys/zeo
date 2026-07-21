# send_rubyfunc_block benchmark (from yjit-bench)

class C
  def ruby_func
    0
  end
end

obj = C.new
i = 0
while i < 5000000
  obj.ruby_func
  obj.ruby_func
  obj.ruby_func
  obj.ruby_func
  obj.ruby_func
  obj.ruby_func
  obj.ruby_func
  obj.ruby_func
  obj.ruby_func
  i = i + 1
end
puts "done"
