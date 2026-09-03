# Ruby names a block after the scope it was WRITTEN in and COUNTS the nesting:
# `block in <base>`, then `block (2 levels) in <base>`, `block (3 levels) in
# <base>`. The base never changes as the nesting grows -- it stays the method
# or body the outermost block sits in.
#
# CLIF composed each block's label out of its ENCLOSING FRAME's label, so a
# nested block read `block in block in <main>`: a spelling ruby never produces,
# and one that grows without bound. A real `def` restarts the count, because
# the method it creates IS the frame however it was written.

def label = caller_locations(1, 1).first.label

# --- at the top level -------------------------------------------------------
p(->() { label }.call)
p(->() { ->() { label }.call }.call)
p(->() { ->() { ->() { label }.call }.call }.call)

# --- under a method ---------------------------------------------------------
class Deep
  def run
    [1].map { [2].map { label } }
  end
end
p Deep.new.run

# --- under a class body -----------------------------------------------------
class Body
  RESULT = [1].map { [2].map { label } }
end
p Body::RESULT

# --- under a singleton-class body -------------------------------------------
class Sing
  class << self
    RESULT = [1].map { [2].map { label } }
  end
end
p Sing.singleton_class::RESULT

# --- a `def` restarts the count ---------------------------------------------
[1].each do
  [2].each do
    def restarted = [3].map { label }
  end
end
p restarted

# --- a `define_method` body IS a block, so its own blocks count from it ------
class Installed
  define_method(:one) { label }
  define_method(:two) { [1].map { label } }
end
p Installed.new.one
p Installed.new.two
__END__
"block in <main>"
"block (2 levels) in <main>"
"block (3 levels) in <main>"
[["block (2 levels) in Deep#run"]]
[["block (2 levels) in <class:Body>"]]
[["block (2 levels) in singleton class"]]
["block in Object#restarted"]
"block in <class:Installed>"
["block (2 levels) in <class:Installed>"]
