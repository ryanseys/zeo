# An inherited factory (`Base.create`) forwards `new(attrs)` with a Hash,
# which unifies the subclass ctor parameter to poly (sp_RbVal) across its
# call sites. The inherited attr_accessor field `id` stays narrow (mrb_int,
# pinned by `@id = 0`). The inlined `self.id = id` setter then writes a poly
# value into the narrow field, which used to emit `mrb_int = sp_RbVal` and
# fail C compilation. The setter now unboxes to the field's concrete type.
class Base
  attr_accessor :id

  def initialize
    @id = 0
  end

  def self.create(attrs = {})
    new(attrs)
  end
end

class Article < Base
  def initialize(id = 0)
    self.id = id
  end
end

puts Article.new(7).id
