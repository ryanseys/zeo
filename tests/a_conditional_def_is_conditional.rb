# A `def` inside a class-body `if` whose guard zeo cannot decide runs only if
# the guard says so -- which means it must claim no compile-time method-table
# row, because that row would answer whether or not the branch ran. What the
# guard installs goes into the runtime overlay instead, and the overlay is what
# answers: it already outranks a class's own table and an inherited one both.
#
# The name still has to REGISTER, so the compile-time machinery that resolves
# `extend` and the method tables can see it. Registering it is not the same as
# promising it.

OFF = ENV.fetch("ZEO_CONDITIONAL_DEF_UNSET", "off") == "on"
ON = ENV.fetch("ZEO_CONDITIONAL_DEF_UNSET", "off") == "off"

# The guard is false, so the method is not there -- not to a call, not to
# `respond_to?`, and not to `instance_methods`.
class Absent
  if OFF
    def only_when_on = :on
  end
end
p Absent.new.respond_to?(:only_when_on)
p Absent.instance_methods(false)
begin
  Absent.new.only_when_on
rescue NoMethodError
  p :no_method
end

# The guard is true, so it is there, by every one of those readings.
class Present
  if ON
    def only_when_on = :on
  end
end
p Present.new.only_when_on
p Present.new.respond_to?(:only_when_on)
p Present.instance_methods(false)

# A conditional `def` does not displace one that always runs: last-def-wins is
# a rule about `def`s that RAN.
class Both
  def which = :always
  if OFF
    def which = :guarded
  end
end
p Both.new.which
p Both.instance_methods(false)

# ...and when the guard IS true, the overlay it writes wins, which is the same
# answer from the other direction.
class BothOn
  def which = :always
  if ON
    def which = :guarded
  end
end
p BothOn.new.which

# Nor does it shadow an ancestor's: with the guard false the inherited body
# answers, which takes the name not claiming a row on the subclass at all.
class Base
  def which = :base
end

class QuietSub < Base
  if OFF
    def which = :sub
  end
end
p QuietSub.new.which
p QuietSub.instance_methods(false)

class LoudSub < Base
  if ON
    def which = :sub
  end
end
p LoudSub.new.which

# Both branches defining the same name is the compat-shim shape, and the taken
# branch is the one that answers.
class Either
  if OFF
    def which = :then_branch
  else
    def which = :else_branch
  end
end
p Either.new.which

# The top-level `class X ... end if cond` form reopens through the same
# machinery (`analyze::try_conditional_reopen` pushes the guard into the class
# body), so it decides the same way. pp.rb's `class Set ... end if set_pp` is
# the shape.
class Reopened
  def base = :base
end

class Reopened
  def added = :added
end if OFF

p Reopened.new.respond_to?(:added)
p Reopened.instance_methods(false)

# Ruby lets a definition carry more than one trailing modifier, and the pair
# pushes into the class body exactly as a single one does. sexp_processor
# closes its whole `Sexp` reopen with two.
class Twice
  def base = :base
end

class Twice
  def added = :added
end unless Twice.new.respond_to? :added if ON

p Twice.new.added
p Twice.instance_methods(false).sort

class TwiceOff
  def base = :base
end

class TwiceOff
  def added = :added
end unless TwiceOff.new.respond_to? :added if OFF

p TwiceOff.new.respond_to?(:added)
p TwiceOff.instance_methods(false)
