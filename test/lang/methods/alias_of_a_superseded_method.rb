# The classic alias-chaining idiom: reopen a class, alias the old body, then
# redefine the method to call the alias. `analyze::add_own_method` keeps one
# row per name (the reopened body replaces the original in `own_methods`), and
# `mro::resolve_aliases` runs after the whole walk, so the alias clones the
# NEW body instead of the one that existed when the alias ran -- `greet` calls
# itself and the process aborts with a native stack overflow.
#
# The fix needs execution order: superseded scopes are never deleted
# (`compiler.scopes` is append-only) and `SiteDef.seq` already records def
# order; aliases must carry their seq and resolve against the newest row
# defined BEFORE them. The sibling divergence -- a CALL placed before the
# reopen also sees the final body -- has its own file,
# `pre_reopen_call_sees_the_final_body.rb`.
class Greeter
  def greet
    "base"
  end
end

class Greeter
  alias_method :old_greet, :greet
  def greet
    "new(#{old_greet})"
  end
end

puts Greeter.new.greet

class Farewell
  def bye
    "bye"
  end
  alias bye_keyword_alias bye
  def bye
    "later(#{bye_keyword_alias})"
  end
end

puts Farewell.new.bye
__END__
new(base)
later(bye)
