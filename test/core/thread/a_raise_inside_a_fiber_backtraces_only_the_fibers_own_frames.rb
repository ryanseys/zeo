# The ec-swap's frame slice, oracle-verified: the fiber starts on a
# FRESH backtrace stack, so a raise through a method inside it sees
# exactly [the method, the fiber block] -- none of main's frames --
# and `caller` at the block top is empty.

def deep_raise
  raise "boom"
end
f = Fiber.new do
  begin
    deep_raise
  rescue => e
    e.backtrace.length
  end
end
p f.resume
f2 = Fiber.new { caller.length }
p f2.resume
__END__
2
0
