# A class first seen through a compact `module A::B::C` definition still
# resolves bare enclosing names from a NESTED reopen's body: CRuby's
# nesting is per definition site, and the nested site carries the full
# chain (rspec-core's formatters.rb opens `module RSpec::Core::Formatters`
# compact while base_formatter.rb nests the same module around
# `Formatters.register self`).
module Outer
  module Core
  end
end
module Outer::Core::Fmt
  def self.register(k) = (@r ||= []) << k
  def self.registered = @r
end
module Outer
  module Core
    module Fmt
      class Base
        Fmt.register self.name
      end
    end
  end
end
p Outer::Core::Fmt.registered
__END__
["Outer::Core::Fmt::Base"]
