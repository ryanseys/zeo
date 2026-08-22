# A later `def` reopening a MODULE reaches BACK: a call written above the
# reopen answers with the reopened body, in every class that mixed the module
# in. The same shape on a plain CLASS is already correct.
#
# `redefs.rs` gives a user class's redefinitions a timeline, and a class's own
# rows are installed at their document position. A module's rows reach a host
# by MATERIALIZATION -- analyze copies them onto every includer once, for the
# whole program -- so the reopen's body is the one that gets copied and there
# is no position to install it at.
#
# The companion file `a_later_def_on_a_builtin_reaches_back.rb` is the same
# question for a builtin, and its header carries the mechanism analysis: a
# global `mark_live()` is the wrong answer (it takes every send in the program
# off the flattened one-probe path), and a per-`(class, name)` switch read at
# the call site is the cheap one. A module differs from a builtin in one way
# that matters here: its hosts are known at compile time, so the switch would
# have to cover each host's materialized copy rather than one native row.
#
# The last line is the shape that already works, kept so a fix cannot close
# this by breaking that.

module M
  def z = "first"
end
class K
  include M
end
p K.new.z

module M
  def z = "second"
end
p K.new.z

class L
  def y = "first"
end
p L.new.y

class L
  def y = "second"
end
p L.new.y
