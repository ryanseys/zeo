# A block body runs only when something yields to it, and how many times is a
# runtime fact -- so a `require` written inside one is not a load this compile
# can perform ahead of time. rack's test helper is the shape that found it:
#
#   def self.separate_testing   # the non-SEPARATE definition
#   end
#   separate_testing do
#     require_relative "../lib/rack/utils"
#   end
#
# The method does not yield, so CRuby never loads the target. Splicing the
# require at the enclosing statement's position loaded it anyway, and
# rack/constants.rb -- already loaded through rack.rb -- ran a second time,
# warning on all 57 of its constants.
#
# Each target below prints when its body runs, so the ORDER and the COUNT are
# both visible, and every marker is read back through `Object.const_defined?`
# rather than `defined?` (a constant a gated unit assigns is one zeo answers
# for at run time, and `defined?` folds).

def self.never_yields
  :no_yield
end

def self.yields_once
  yield
end

# 1. The block never runs, so neither does its require.
never_yields do
  require_relative "a_require_in_a_block_runs_when_the_block_runs/never"
end
p Object.const_defined?(:NEVER_MARKER)

# 2. A block that does run loads its target at the point it runs, after the
#    statement before it.
puts "before once"
yields_once do
  require_relative "a_require_in_a_block_runs_when_the_block_runs/once"
end
puts "after once"
p Object.const_defined?(:ONCE_MARKER)

# 3. A lambda body is a block too: nothing loads until it is called.
loader = -> { require_relative "a_require_in_a_block_runs_when_the_block_runs/lazy" }
p Object.const_defined?(:LAZY_MARKER)
loader.call
p Object.const_defined?(:LAZY_MARKER)

# 4. A block that runs many times loads its target once, and `require_relative`
#    answers false from the second run on.
answers = [1, 2, 3].map do
  require_relative "a_require_in_a_block_runs_when_the_block_runs/repeat"
end
p answers

# 5. A block inside a class body is the same case -- `class_eval` does yield,
#    so this one loads.
class Host
  [:only].each do
    require_relative "a_require_in_a_block_runs_when_the_block_runs/in_body"
  end
end
p Holder::IN_BODY_MARKER

# 6. The rack shape end to end: a require reached only through a block that
#    never yields leaves the file unloaded, and a later require of it from a
#    position that DOES run loads it exactly once.
never_yields do
  require_relative "a_require_in_a_block_runs_when_the_block_runs/never"
end
p require_relative("a_require_in_a_block_runs_when_the_block_runs/never")
p NEVER_MARKER
p require_relative("a_require_in_a_block_runs_when_the_block_runs/never")
__END__
false
before once
once.rb ran
after once
true
false
lazy.rb ran
true
repeat.rb ran
[true, false, false]
in_body.rb ran
:in_body
never.rb ran
true
:never
false
