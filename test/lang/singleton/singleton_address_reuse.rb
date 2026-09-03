# A singleton method is stored against its receiver's heap identity. Once the
# receiver dies, an unrelated later object must not inherit it because the
# allocator handed out the same address again.

def with_singleton
  s = "victim"
  def s.shout = "SHOUT"
  s
end

module Loud
  def volume = 11
end

def with_extend
  a = [1, 2, 3]
  a.extend(Loud)
  a
end

def with_define_singleton
  h = { k: 1 }
  h.define_singleton_method(:tag) { "tagged" }
  h
end

200.times { with_singleton }
200.times { with_extend }
200.times { with_define_singleton }
GC.start

strays = [0, 0, 0]
2000.times do
  strays[0] += 1 if ("x" * 6).respond_to?(:shout)
  strays[1] += 1 if [9, 8, 7].respond_to?(:volume)
  strays[2] += 1 if { j: 2 }.respond_to?(:tag)
end
p strays

# The live receivers still answer, which is what the pin has to preserve.
p with_singleton.shout
p with_extend.volume
p with_define_singleton.tag

# `dup` drops singletons -- a fresh object is a fresh identity.
kept = with_singleton
p kept.dup.respond_to?(:shout)
p kept.respond_to?(:shout)
__END__
[0, 0, 0]
"SHOUT"
11
"tagged"
false
true
