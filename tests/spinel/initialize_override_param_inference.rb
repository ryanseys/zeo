# A subclass `initialize` override repurposes its positional parameters; a
# parent `initialize` parameter default (e.g. `opts = nil`) must not widen an
# unrelated override parameter to poly. Each case checks the override param
# carries its value through, and that string params stay concrete.

# Minimal repro: parent nil-default, child int param.
class Base1
  def initialize(opts = nil)
    @id = 0
  end
end
class Article1 < Base1
  attr_reader :id
  def initialize(id = 0)
    @id = id
  end
end
a1 = Article1.new(7)
puts a1.class.to_s
puts a1.id

# Multi-param override: int + two string params, none poisoned.
class Base2
  def initialize(opts = nil)
    @id = 0
  end
end
class Article2 < Base2
  attr_reader :id, :title, :body
  def initialize(id = 0, title = "", body = "")
    @id = id
    @title = title
    @body = body
  end
end
a2 = Article2.new(7, "t", "b")
puts a2.id
puts a2.title
puts a2.body

# Parent concrete default (control: must keep working).
class Base3
  def initialize(opts = 0)
    @id = 0
  end
end
class Article3 < Base3
  attr_reader :id
  def initialize(id = 0)
    @id = id
  end
end
puts Article3.new(7).id

# Explicit super() with no forwarding.
class Base4
  def initialize(opts = nil)
    @id = 0
  end
end
class Article4 < Base4
  attr_reader :id
  def initialize(id = 0)
    super()
    @id = id
  end
end
puts Article4.new(7).id
