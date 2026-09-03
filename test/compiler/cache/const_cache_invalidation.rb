WHICH = "first"

def read_which = WHICH

# Warm the site, then reassign the same constant. Ruby warns and rebinds; the
# cached read must not survive the rebinding.
p read_which
Object.send(:remove_const, :WHICH)
WHICH = "second"
p read_which

module Shelf
  ITEM = "shelf"
  def self.item = ITEM
end
p Shelf.item
Shelf.send(:remove_const, :ITEM)
Shelf.const_set(:ITEM, "restocked")
p Shelf.item

# A constant read that MISSES caches nothing, so defining the name afterwards
# has to take effect.
def read_late = LATE
begin
  read_late
rescue NameError => e
  p e.message
end
LATE = 42
p read_late

# Removing a constant after the site has cached it must raise again.
def read_gone = GONE
GONE = 1
p read_gone
Object.send(:remove_const, :GONE)
begin
  read_gone
rescue NameError => e
  p e.message
end

# A nested class named only once it is bound to a constant becomes reachable
# through that namespace.
module Box; end
def read_inner = Box::Inner
begin
  read_inner
rescue NameError => e
  p e.class
end
Box.const_set(:Inner, Class.new)
p read_inner.name
p read_inner.name

# Threads share one constant table, so a value written by one is what the
# other's cached site must answer with.
SHARED = "before"
def read_shared = SHARED
p read_shared
Thread.new do
  Object.send(:remove_const, :SHARED)
  Object.const_set(:SHARED, "after")
end.join
p read_shared
__END__
"first"
"second"
"shelf"
"restocked"
"uninitialized constant LATE"
42
1
"uninitialized constant GONE"
NameError
"Box::Inner"
"Box::Inner"
"before"
"after"
