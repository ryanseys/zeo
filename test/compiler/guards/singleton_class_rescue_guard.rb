# A `rescue` guarding a statement inside `class << obj`, the per-instance
# singleton class.
#
# The mapping accepts a fixed set of items and rejects the rest, because `self`
# in that body is the object's singleton class and a statement that consults it
# cannot simply run in the enclosing scope. A `begin/rescue` -- and the modifier
# form, which lowers to the same node -- consults nothing itself: it only
# guards. So the guarded statements map exactly as they would unguarded, and
# the guard is kept.
#
# tins writes it: `class << modul; remove_method :now rescue nil; end`, where
# the `rescue` is load-bearing because `remove_method` raises NameError unless
# the method is defined at that exact level.

module Clock
  def self.now
    "real now"
  end
end

p Clock.now

class << Clock
  alias really_now now

  # The modifier form. The second one raises NameError, and the guard is what
  # the gem relies on to survive it.
  remove_method :now rescue nil
  remove_method :never_was_defined rescue nil

  def now
    "frozen now"
  end
end

p Clock.now
p Clock.really_now

# The explicit form, and nested inside a conditional the mapping already
# recurses through, so the two compose.
obj = Object.new

class << obj
  if RUBY_VERSION > "2.0"
    begin
      remove_method :inspect
    rescue NameError
      nil
    end

    def to_s
      "custom"
    end
  end
end

p obj.to_s
__END__
"real now"
"frozen now"
"real now"
"custom"
