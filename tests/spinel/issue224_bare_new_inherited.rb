# Issue #224: bare `new` inside an inherited class method body must
# resolve to the *calling subclass*, not the parent class that
# defined the method. Sister to issue #208 (which fixed cls method
# dispatch up the inheritance chain); once `Article.create` runs
# `Base.create`'s body, the bare `new` *inside* that body should
# also dispatch to Article's constructor.
#
# Pre-fix the basic case warned "cannot resolve call to 'x' on
# obj_Base" and emitted 0 (because new resolved statically to
# Base.new, returning an obj_Base which has no `x` method); the
# variant with attrs args produced a C-level type mismatch
# (`sp_Base_new(lv_attrs)` calling Base's 0-arg new with an extra
# pointer arg).

# Basic: bare `new`, no args.
class Base
  def self.create
    new
  end
end

class Article < Base
  def initialize
    @x = "hello"
  end
  def x; @x; end
end

puts Article.create.x                  # hello

# Variant: bare `new(args)`, attrs hash. The cls method param `attrs`
# widens from the call site `Article.create({title: ...})` to a
# typed hash; the bare `new(attrs)` propagates that ptype to
# Article's `initialize` so the synthetic `sp_Article_new` signature
# matches.
class Base2
  def self.create(attrs)
    instance = new(attrs)
    instance
  end
end

class Article2 < Base2
  def initialize(attrs)
    @title = attrs[:title]
  end
  def title; @title; end
end

puts Article2.create({title: "hello"}).title     # hello

# Multi-level: bare `new` resolves to the leaf class.
class Mid < Base; end
class Deep < Mid
  def initialize
    @x = "deep"
  end
  def x; @x; end
end
puts Deep.create.x                     # deep

# Direct call on the defining class still works (Base has no
# initialize so Article-via-Base.create wouldn't make sense, but
# Article2.create directly uses Base2.create's body via the
# synthetic copy and exercises the same path twice).
puts Article2.create({title: "world"}).title     # world
