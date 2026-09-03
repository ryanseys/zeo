# A class REOPENING that only ADDS methods, guarded by a runtime-undecidable
# condition (`... end if enabled`). zeo pushes the guard into the class body
# (`class Widget; if enabled; def extra; ...`) so the method is defined
# conditionally at runtime and still registered for reflection -- matching real
# Ruby (this is the pp.rb `class Set ... end if set_pp` monkeypatch idiom).
class Widget
  def base
    "base"
  end
end

enabled = [true].sample # deterministic (single element) but not statically foldable

class Widget
  def extra
    "extra"
  end
end if enabled

w = Widget.new
puts w.base
puts w.extra
puts Widget.instance_method(:extra).name
__END__
base
extra
extra
