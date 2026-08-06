# A method whose name is `self`.
#
# Ruby allows it: `def self(token)` with an argument list defines an ordinary
# instance method called `self`, reachable through `send`. The `parser` gem
# does this in its AST builder (parser/lib/parser/builders/default.rb):
#
#   def self(token)
#     n0(:self, ...)
#   end
#
# zeo turns a method name into a Rust identifier, and `self` is a Rust path
# keyword with no raw-identifier form -- `r#self` does not exist. Today
# `safe_ident` asserts it can never be handed one, so this arrives as an
# internal panic rather than a clean diagnostic naming the construct.
#
# Found by `cargo xtask gem-probe`, which records `parser` as compiler-panic.

class Builder
  def self(token)
    "made #{token}"
  end
end

puts Builder.new.send(:self, "node")
p Builder.instance_methods(false)
