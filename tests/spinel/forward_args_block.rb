# `...` forwards a block as well as positional/keyword args. The forwarder is a
# real function, so a literal block is redirected straight to the (yielding or
# &block) target it forwards to, with the call-site args coming along.

def y_target(a, b)
  yield(a + b)
end
def fwd(...)
  y_target(...)
end
puts(fwd(3, 4) { |s| s * 2 })          # 14

# Chained forwarders, block threaded to the final yielder.
def g(a)
  yield(a * 10)
end
def f(...)
  g(...)
end
def e(...)
  f(...)
end
puts(e(5) { |x| x + 1 })               # 51

# Forward to a target with an explicit &block param.
def block_target(a, &blk)
  blk.call(a) + blk.call(a)
end
def fwd_bt(...)
  block_target(...)
end
puts(fwd_bt(7) { |n| n * n })          # 98

# Instance-method forwarding with a block.
class C
  def base(a)
    yield(a)
  end
  def relay(...)
    base(...)
  end
end
puts(C.new.relay(9) { |v| v - 3 })     # 6

# Forward positional + keyword args together with a block.
def kw_target(a, mult:)
  yield(a * mult)
end
def fwd_kw(...)
  kw_target(...)
end
puts(fwd_kw(3, mult: 4) { |r| r + 100 })   # 112
