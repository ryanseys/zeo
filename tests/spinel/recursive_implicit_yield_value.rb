# A self-recursive method that uses implicit `yield` and forwards its
# block down the recursion (`countdown(n - 1) { yield }`) carries the
# block's value back at the block's real type. Such methods are lowered
# to an &block (sp_Proc) form: yield -> sp_proc_call(__yblk__), the
# literal block is forwarded as a proc, and the nested `{ yield }`
# captures the synthetic block param.

def countdown(n)
  return yield if n <= 0
  countdown(n - 1) { yield }
end
puts countdown(2) { "done" }   # string block
puts countdown(3) { 42 }       # int block at the same method
puts countdown(0) { "base" }   # base case taken immediately

# Non-recursive yield methods are unaffected (still inlined).
def twice
  yield
  yield
end
twice { puts "tick" }

def give
  yield 5
end
puts give { |x| x + 1 }

# Instance-method implicit-yield recursion.
class Counter
  def countdown(n)
    return yield if n <= 0
    countdown(n - 1) { yield }
  end
end
puts Counter.new.countdown(2) { "inst" }
