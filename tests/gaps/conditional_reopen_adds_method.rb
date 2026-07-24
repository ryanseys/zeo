# A class REOPENING that only ADDS methods, guarded by a runtime-undecidable
# condition (`... end if enabled`). zeo's analyze rejects any class/module
# definition inside a non-compile-time-decidable top-level `if`; but a reopening
# that just adds methods to an ALREADY-defined class is safe to run conditionally
# at that document position, exactly as real Ruby does (this is the
# pp.rb `class Set ... end if set_pp` monkeypatch idiom).
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
