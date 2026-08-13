# `eval` of a literal string that defines a `class`/`module` at its own top
# level. CRuby parses the snippet at run time and the definition is ordinary.
#
# zeo's zero-cost path parses the literal at COMPILE time and inlines it as
# `HirNode::Eval(body)` -- but the registration walk never unwraps an `Eval`
# node, so a `class` written there reached codegen with no class-body site and
# died as "a position the analyze walk doesn't register". That is the same
# hole `reject_top_level_defs` already covers for a top-level `def`, and it
# takes the same exit: the snippet falls through to the RUNTIME eval VM
# instead, so the program compiles and the limitation surfaces where CRuby's
# own eval errors surface -- at the call, catchably.
#
# The remaining divergence is that the eval VM has no `class`/`module` node
# yet, so the call raises `NotImplementedError` where ruby defines the module.
# testability-driver is the shape: an `eval('module
# TDriver_Error_Recovery_ATS4 ... end')` inside a method, behind a settings
# guard that is normally false -- so its three gems now compile, and only a
# program that really takes that branch meets this.
#
# SHAPE OF A FIX: a `ClassDef`/`DefMethod` node in the runtime eval VM, which
# needs a runtime-minted class rather than a compile-time-registered one.
def make
  eval('module Made
    WHO = "made"
    def self.hi = "made hi"
  end')
end

make
p [Made::WHO, Made.hi, Made.class]

# ...and the class form, with a superclass the enclosing program named.
class Base
  def hi = "base"
end

eval('class Derived < Base
  def hi = "derived (#{super})"
end')
p Derived.new.hi
