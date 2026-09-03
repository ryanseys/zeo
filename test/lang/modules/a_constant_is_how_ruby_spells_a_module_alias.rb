# Ruby has no separate alias form for a class or module: naming one twice IS
# assigning its value to a second constant, and the second name then works
# everywhere the first does -- `include`, a superclass clause, `.new`. oauth2
# keeps `OAuth2::FilteredAttributes = OAuth2::AUTH_SANITIZER::FilteredAttributes`
# as a permanent public alias after moving the implementation to another gem;
# celluloid-io writes `Constants = ::Socket::Constants` and includes that.

module Vendor
  module Sanitizer
    module FilteredAttributes
      def filtered = :filtered
    end

    class Base
      def kind = :base
    end
  end
end

module App
  # The alias sits in the ENCLOSING scope, and the class body one level in
  # reads it -- ruby's lexical lookup, and where the scope-blind version of
  # this search used to find the wrong module.
  FilteredAttributes = Vendor::Sanitizer::FilteredAttributes

  class Authenticator
    include FilteredAttributes
  end

  # ... and as a superclass, anchored at the top level with `::`.
  Base = ::Vendor::Sanitizer::Base
  class Client < Base
    def kind = [:client, super]
  end
end

p App::Authenticator.new.filtered
p App::Client.new.kind
p App::Authenticator.ancestors.include?(Vendor::Sanitizer::FilteredAttributes)
p App::Client.superclass.equal?(Vendor::Sanitizer::Base)

# An alias OF an alias resolves through.
module Deeper
  Same = App::Base
  class Grand < Same
  end
end
p Deeper::Grand.superclass.equal?(Vendor::Sanitizer::Base)
__END__
:filtered
[:client, :base]
true
true
true
