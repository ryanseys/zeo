# `C.define_singleton_method(:m) { }` written at STATEMENT position is a
# runtime event with a place in the program. Lowering desugared it into an
# inline `def self.m` reopen, and last-`def`-wins then made both call sites
# answer the new body -- so the method reached back in time.
class Later
  def self.mode = :compiled
end

p Later.mode
Later.define_singleton_method(:mode) { :runtime }
p Later.mode
p Later.singleton_methods(false).sort

# A name the class never defined is added, not replaced.
Later.define_singleton_method(:fresh) { :new_one }
p Later.fresh
p Later.respond_to?(:fresh)

# The block is a real closure over its writing scope.
tag = :captured
Later.define_singleton_method(:tagged) { tag }
p Later.tagged

# Arguments bind through the block's parameter list.
Later.define_singleton_method(:twice) { |n| n * 2 }
p Later.twice(21)

# The IN-BODY spelling still installs at compile time, where nothing can
# observe a before.
class InBody
  define_singleton_method(:mode) { :from_body }
end
p InBody.mode
p InBody.singleton_methods(false)

# `self.` inside a body means the same thing.
class SelfBody
  self.define_singleton_method(:mode) { :from_self }
end
p SelfBody.mode
__END__
:compiled
:runtime
[:mode]
:new_one
true
:captured
42
:from_body
[:mode]
:from_self
