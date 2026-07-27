# `x ||= v` on a method-local with no earlier assignment, and a tail assignment
# as a method's value.
#
# The or-write emitter treated every non-poly local as already truthy, so the
# assignment was dropped entirely -- the variable kept its declaration's zero.
# And a tail local write returned the return type's default rather than the
# value just assigned, so even an or-write that DID run was invisible.

class Crash
  def to_s
    text ||= ["Usage:", "foo"].join(" ")
  end
end
p Crash.new.to_s

# every representation: pointer slots test NULL, the sentinel-carrying scalars
# are declared with their nil sentinel because nothing else ever assigns them
def gi; x ||= 5; x; end
def gf; x ||= 1.5; x; end
def gs; x ||= "s"; x; end
def ga; x ||= [1, 2]; x; end
def gh; x ||= { "a" => 1 }; x["a"]; end   # not the Hash itself: #inspect spacing moves between Ruby versions
def gb; x ||= true; x; end
def gy; x ||= :sym; x; end
p gi, gf, gs, ga, gh, gb, gy

# a definite assignment first: the or-write is the no-op it always was
def hi; x = 3; x ||= 5; x; end
def hs; x = "q"; x ||= "s"; x; end
def hn; x = nil; x ||= "z"; x; end
p hi, hs, hn

# a tail write is the method's value, whatever the shape
def ti; x = 5; end
def ts; x = "hi"; end
def ta; x = [1]; end
def to; x ||= "o"; end
p ti, ts, ta, to

# the or-write must not re-fire once the slot holds something, and must fire
# again for each fresh block-local
def loops
  seen = []
  i = 0
  while i < 3
    memo ||= "iter#{i}"
    seen.push(memo)
    i += 1
  end
  seen
end
p loops

def per_block
  out = []
  3.times do |n|
    fresh ||= "b#{n}"
    out.push(fresh)
  end
  out
end
p per_block

# the value expression must not run when the slot is already set: the whole
# point of ||= is that the right-hand side is skipped
$calls = 0
def costly
  $calls += 1
  "made"
end

def once
  v = nil
  2.times { v ||= costly }
  v
end
p once
p $calls
