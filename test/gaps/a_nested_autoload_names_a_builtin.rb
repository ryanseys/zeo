# frozen_string_literal: true

# An `autoload` naming a GATED builtin feature, written in a class body.
# The bare `Digest` read resolves statically to `::Digest` and touches only
# the top level's autoloads, never `Store`'s, so the gate never opens and the
# constant is missing. The top-level half already matches.

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
