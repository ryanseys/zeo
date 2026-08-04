# The receiver temporary of a polymorphic send (the `switch (cls_id)`
# dispatch) must be GC-rooted: it can be the only live reference to the
# receiver, methods do not root their own self, and a collection triggered
# by allocation inside the callee then frees the receiver while its method
# is still running (#3476). Each go/add below collects and then allocates
# before reading an ivar, so the freed receiver's storage is handed out
# again and the stale read is a crash rather than a value that happens to
# survive -- without the second step this passes on Linux either way and
# only ASAN or macOS sees the use-after-free.
class Holder
  def initialize(params = {})
    @params = params
  end

  def go
    GC.start                                   # frees the unrooted receiver
    junk = Array.new(64) { |j| Holder.new(k: [j]) }   # hands its slot out again
    (@params[:k] || []).length + junk.length - 64
  end

  def add(x)
    GC.start
    junk = Array.new(64) { |j| Holder.new(k: [j]) }
    (@params[:k] || []).length + x + junk.length - 64
  end
end

class Other
  def go
    0
  end

  def add(x)
    x
  end
end

def pick
  return Other.new if false  # gives the call site a union receiver type
  Holder.new(k: [1, 2, 3])
end

r = 0
i = 0
while i < 50
  r += pick.go               # receiver lives only as the dispatch temporary
  r += pick.add(10)          # same, through the with-args dispatch
  i += 1
end
puts "ok r=#{r}"
