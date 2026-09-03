# A local first assigned inside a `begin` body is still in scope in the
# `rescue`/`ensure` clauses and after the whole `begin` -- one Ruby scope, even
# though each clause runs on its own path.
class Box
  def initialize(v) = @v = v
  def v = @v
  def close = "closed #{@v}"
end

def read_in_ensure
  begin
    b = Box.new(1)
    b.v
  ensure
    puts b.close
  end
end
p read_in_ensure

def read_after_rescue
  begin
    c = Box.new(2)
  rescue
    nil
  end
  c.v
end
p read_after_rescue

def assigned_in_rescue
  begin
    raise "boom"
  rescue
    d = Box.new(3)
  end
  d.v
end
p assigned_in_rescue

# Reassigning inside the body must be visible outside it, not shadowed by a
# binding confined to the body.
def reassigned_inside
  e = Box.new(4)
  begin
    e = Box.new(5)
  ensure
    nil
  end
  e.v
end
p reassigned_inside

# ...and a multi-assignment's targets are locals for this purpose too.
def multi_assigned_inside
  begin
    f, g = Box.new(6), Box.new(7)
  ensure
    nil
  end
  [f.v, g.v]
end
p multi_assigned_inside

# A local the body never reaches is nil in the ensure, not undefined.
def never_assigned
  begin
    raise "boom"
    h = Box.new(8)
  rescue
    p defined?(h)
    p h
  end
end
never_assigned
__END__
closed 1
1
2
3
5
[6, 7]
"local-variable"
nil
