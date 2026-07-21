# #563 (Sam Ruby). `self[k]` and `self[k] = v` from inside a
# parent-defined method should dispatch to the subclass
# override at runtime. Sibling of bare_imeth_subclass_dispatch.rb
# but for the operator forms; pre-fix the dispatch resolved
# statically to the parent's operator stub, ignoring the
# subclass override.
#
# Roundhouse trigger: ActiveRecord::Base#fill_timestamps does
# `self[:updated_at] = now if cols.include?(:updated_at)`,
# expecting the dispatch to land on the subclass's column
# routing. Pre-fix every model raised NotImplementedError.

class Base
  def [](name)
    raise NotImplementedError
  end

  def []=(name, value)
    raise NotImplementedError
  end

  def fetch
    self[:foo]
  end

  def fill
    self[:foo] = "bar"
  end
end

class Article < Base
  def [](name); "result"; end
  def []=(name, value); @foo = value; end
  def foo; @foo; end
end

a = Article.new
puts a.fetch
a.fill
puts a.foo
