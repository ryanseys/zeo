# A superclass clause resolves through the ENCLOSING class's ancestry, and
# an inherited constant may be an ALIAS of a class defined elsewhere. rss's
# maker layer is the shape: `AuthorsBase = ChannelBase::AuthorsBase` inside
# ItemBase, read by `class Authors < AuthorsBase` under `Item < ItemBase`.
module M
  class Base
    class AuthorsBase
      def kind
        :authors_base
      end
    end
  end
  class Channel < Base
    class Authors < AuthorsBase; end
  end
  class ItemBase
    AuthorsBase = Base::AuthorsBase
  end
  class Item < ItemBase
    class Authors < AuthorsBase; end
  end
end
p M::Channel::Authors.superclass
p M::Item::Authors.superclass
p M::Item::Authors.new.kind
p M::Channel::Authors.new.kind
__END__
M::Base::AuthorsBase
M::Base::AuthorsBase
:authors_base
:authors_base
