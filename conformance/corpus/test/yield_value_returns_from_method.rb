def f
  yield
end

def factory_init(size)
  free = []
  i = 0
  while i < size
    free.push(yield)
    i += 1
  end
  free
end

def double_yield
  yield * 2
end

puts f { 99 }

result = factory_init(3) { 42 }
puts result.length
puts result[0]
puts result[1]
puts result[2]

puts double_yield { 21 }
