# frozen_string_literal: true

# An `autoload` naming a GATED builtin feature, at the top level and in a
# class body. A bare `Digest` read inside the class runs the class's own
# autoload, which requires the feature, and then names the builtin.

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
