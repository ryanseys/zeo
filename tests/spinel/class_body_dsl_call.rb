# self in a class body is the class, so a receiver-less call there naming a class
# method is a real call. It was treated as a declaration macro and skipped, so
# every declarative class-body DSL recorded nothing and the read afterwards
# answered nil, with no complaint from either the compiler or the run (#4051).
# The inherited case needs the second half of the fix: the DSL method has to be
# specialized for the subclass, or it writes the base class's class-level ivar --
# including when it never names one itself and reaches it through a sibling.
class Base
  def self.key(k = nil)
    @key = k if k
    @key
  end
end

class A < Base
  key :a
end
class B < Base
  key :b
end
p A.key
p B.key
p Base.key

class Own
  def self.tag(t = nil)
    @tag = t if t
    @tag
  end
  tag :own
end
p Own.tag

# The DSL method names no ivar itself: it reaches one through a sibling class
# method the subclass also inherits unchanged.
class Model
  def self.fields
    @fields ||= []
  end
  def self.field(n, type = :string)
    fields.push([n, type])
  end
end

class User < Model
  field :id, :int
  field :name
end
class Post < Model
  field :title
end
p User.fields
p Post.fields
p Model.fields

module Notes
  def self.note(s); (@notes ||= []).push(s); end
  def self.notes; @notes || []; end
  note "from the module body"
end
p Notes.notes
