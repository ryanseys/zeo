# An Object reopen is top-level `def` by another name: its methods
# land on arena slot 0, materialize into every user class, and
# dispatch on the runtime `main` object at top-level call sites.

class Object
  def double(x)
    x * 2
  end
end
puts double(21)
class Widget
  def go
    double(4)
  end
end
puts Widget.new.go
__END__
42
8
