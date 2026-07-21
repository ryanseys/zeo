def foo
  yield
end

def bar
  foo { return 1 }
end

total = 0
i = 0
while i < 200000
  total = total + bar
  total = total + bar
  total = total + bar
  total = total + bar
  total = total + bar
  total = total + bar
  total = total + bar
  total = total + bar
  total = total + bar
  total = total + bar
  i = i + 1
end
puts total
puts "done"
