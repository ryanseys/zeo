# Empty `[]` literal as parameter — deferred element-type resolution
# must converge across caller / param / body. Issues #58 (top-level
# def + instance method) and #84 (inherited dispatch). Was four
# tests; class names don't collide so the merge is straight
# concatenation with shared helpers.

# === Top-level def, [] passed directly at call site ===
def push_floats(buf)
  buf.push(1.5)
  buf.push(2.5)
  buf
end
result_pf = push_floats([])
puts result_pf[0]   # 1.5
puts result_pf[1]   # 2.5

# === Top-level def, [] stored in local first ===
def collect_names(buf)
  buf.push("alpha")
  buf.push("beta")
end
names = []
collect_names(names)
puts names[0]       # alpha
puts names[1]       # beta
puts names.length   # 2

# === Instance method receiving [] — same deferred path ===
class Recorder
  def record(buf, name)
    buf.push(name)
    buf.push(name + "!")
  end
end
r = Recorder.new
log = []
r.record(log, "go")
r.record(log, "stop")
puts log.length     # 4
puts log[0]         # go
puts log[1]         # go!
puts log[2]         # stop
puts log[3]         # stop!

# === Inherited dispatch (#84): caller-side inference must walk
# the inheritance chain to update the *parent's* @cls_meth_ptypes
# when cls_find_method_direct misses on the child. ===
class Base
  def add_to(buf)
    buf.push("hi")
  end
end
class Child < Base
end
names1 = []
Child.new.add_to(names1)
names1.push("more")
puts names1[0]      # hi
puts names1[1]      # more

# Two-deep inheritance.
class Grandchild < Child
end
names2 = []
Grandchild.new.add_to(names2)
names2.push("again")
puts names2[0]      # hi
puts names2[1]      # again

# Class that overrides the inherited slot AND inherits a different
# defaulted method. The promotion must hit Base2's slot for the
# inherited `also_add` call, not Mixed's.
class Base2
  def also_add(buf)
    buf.push("base2")
  end
end
class Mixed < Base2
  def add_to(buf)
    buf.push(42)
  end
end

ints = []
Mixed.new.add_to(ints)
ints.push(7)
puts ints[0]        # 42
puts ints[1]        # 7

strs = []
Mixed.new.also_add(strs)
strs.push("more")
puts strs[0]        # base2
puts strs[1]        # more
