# A method whose name is `self`. Ruby allows it: `def self(token)` with an
# argument list defines an ordinary instance method called `self`, reachable
# through `send`. The `parser` gem's AST builder does this.
#
# `self` is a Rust path keyword with no raw-identifier form -- `r#self` does
# not exist -- so it gets a mangled spelling of its own.

class Builder
  def self(token)
    "made #{token}"
  end
end

puts Builder.new.send(:self, "node")
p Builder.instance_methods(false)
__END__
made node
[:self]
