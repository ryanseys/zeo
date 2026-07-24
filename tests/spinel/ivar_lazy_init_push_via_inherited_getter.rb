# #451: cross-class sequel to #430. The lazy-init getter
# `def errors; @errors = [] if @errors.nil?; @errors; end` is
# defined on a parent class; the push `errors << "..."` happens
# from a child class method. Before the fix, scan_writer_calls
# only inspected @current_class_idx for the getter, so the
# parent's @errors stayed `sp_IntArray *` and the child's
# String push failed C compile.

class Base
  def errors
    @errors = [] if @errors.nil?
    @errors
  end
end

class Foo < Base
  def add_error(msg)
    errors << "#{msg} bad"
  end
end

f = Foo.new
f.add_error("name")
f.add_error("body")
puts f.errors.length
puts f.errors[0]
puts f.errors[1]

# Two levels of inheritance — getter on grandparent.
class Tracker
  def events
    @events = [] if @events.nil?
    @events
  end
end

class TrackerMid < Tracker
end

class TrackerLeaf < TrackerMid
  def record(name)
    events << name
  end
end

t = TrackerLeaf.new
t.record("a")
t.record("b")
puts t.events.length
puts t.events.join(",")

# Same-class case (post-#430) still works.
class Same
  def items
    @items = [] if @items.nil?
    @items
  end

  def add(s)
    items << s
  end
end

s = Same.new
s.add("x")
s.add("y")
puts s.items.join(":")
