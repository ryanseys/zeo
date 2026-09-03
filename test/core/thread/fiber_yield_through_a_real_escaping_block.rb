# The hardest suspension shape: Fiber.yield fires inside a real Proc
# (the block each_twice invokes), unwinding through the Proc's closure
# frame AND each_twice's own method frame to the resumer.

class Iter
  def each_twice
    yield 1
    yield 2
  end
end
it = Iter.new
f = Fiber.new do
  it.each_twice do |n|
    Fiber.yield n
  end
  :done
end
puts f.resume
puts f.resume
puts f.resume
__END__
1
2
done
