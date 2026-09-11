# frozen_string_literal: true

# A bare constant read inside a class runs that class's own `autoload` of
# the name, and an enclosing class's, innermost first, before the name
# resolves -- also when the name is a GATED builtin that only the autoload's
# `require` brings in. `defined?` sees the registration without loading.
module Outer
  autoload :Digest, "digest"

  class Inner
    def digest_name = Digest.name
    def defined_digest = defined?(Digest)
  end
end

class Store
  autoload :JSON, "json"

  def self.dump = JSON.generate([1])
end

p Outer::Inner.new.defined_digest
p Outer::Inner.new.digest_name
p Store.dump
__END__
"constant"
"Digest"
"[1]"
