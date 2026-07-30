# `defined?(Const)` answers from DOCUMENT ORDER: ruby knows a constant only
# once its `class` statement or its assignment has executed, where a
# whole-program view finds the definition wherever in the file it is written.
# `analyze::mro::index_document_order` numbers the statements in execution
# order and records where each constant starts answering, so the fold commits
# only when the definition provably precedes the query.
#
# The numbering deliberately stops at a `def`, a lambda and a block body --
# those run when they are CALLED, which is no position at all -- and a query
# or a definition it cannot place keeps the whole-program answer.
p defined?(Later)
class Later; end
p defined?(Later)

module Wrapper
  p defined?(Inner)
  class Inner; end
  p defined?(Inner)
end

# The same question through a conditional, which is the shape that matters --
# a compat shim guarding on a constant its own file defines further down.
if defined?(Sentinel)
  p :early
else
  p :not_yet
end
Sentinel = 1
p defined?(Sentinel)

# The mirror image, and a second cause: a TOP-LEVEL value constant used to
# answer nil even AFTER its assignment. `directly_defines_const` reads a
# class's `class_body_stmts`, and a top-level `NAME = ...` sits in the main
# statement stream instead, belonging to no class body -- so nothing claimed
# it for `Object`, where ruby puts it. Only STATEMENT-level writes are
# claimed, the same rule a class body follows: `NAME = 1 if cond` names an
# owner it may never get.
AT_TOP_LEVEL = 7
p defined?(AT_TOP_LEVEL)

# A constant is defined once its VALUE has been computed, so a self-reference
# in the initializer still answers nil.
SELF_REF = defined?(SELF_REF)
p SELF_REF

# A query inside a method body has no document position -- it runs whenever
# the method is called -- so it keeps answering from the whole program.
def probe = defined?(DEFINED_LATER)
DEFINED_LATER = :here
p probe
