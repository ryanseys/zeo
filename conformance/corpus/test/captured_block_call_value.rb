# A nested proc literal can capture an enclosing method's &block (an
# sp_Proc param) and call it, and the value flows back at the block's
# real type. Covers the value carried through the proc-var indirection
# (`p = proc { blk.call }; p.call`) for int / string / argument blocks,
# at top level and inside an instance method.

# top-level, int block
def f_int(&blk)
  p = proc { blk.call }
  p.call
end
puts f_int { 7 }

# top-level, string block: the value rides the int slot and reads back
# as a string (typed through the p.call -> blk.call indirection).
def f_str(&blk)
  p = proc { blk.call }
  p.call
end
puts f_str { "hi" }

# top-level, block with an argument
def f_arg(&blk)
  p = proc { blk.call(10) }
  p.call
end
puts f_arg { |x| x + 5 }

# a plain proc local inside an instance method (no &block) is typed proc
class Box
  def make
    p = proc { 42 }
    p.call
  end
end
puts Box.new.make

# instance method capturing its &block through a proc local, string block
class Runner
  def run(&blk)
    p = proc { blk.call }
    p.call
  end
end
puts Runner.new.run { "inst" }
