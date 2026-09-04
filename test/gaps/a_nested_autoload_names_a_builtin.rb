# frozen_string_literal: true

# Two things an `autoload` naming a GATED builtin feature gets wrong.
#
# A top-level one does not mark the feature LOADED, so a later `require`
# of it answers true where ruby answers false. One written in a class
# body never opens the gate at all, so the constant is missing.

autoload :JSON, "json"
p require("json")

class Store
  autoload :Digest, "digest"

  def digest = Digest.name
end

p Store.new.digest
__END__
true
"Digest"
