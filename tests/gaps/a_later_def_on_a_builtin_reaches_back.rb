# A later `def` reopening a BUILTIN class reaches BACK: a call written above
# the reopen answers with the reopened body. `redefs.rs` gives a user class's
# redefinitions a timeline, but a builtin's FIRST definition is a native row
# with no `Scope`, so there is no `method_history` entry to order against and
# the reopen is installed whole-program at startup.
p [1, 2].take_while { true }
class Array
  def take_while
    :array_own
  end
end
p [1, 2].take_while { true }
