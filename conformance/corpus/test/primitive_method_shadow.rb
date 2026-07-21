# Issue #302: a user-defined class method whose name matches a
# primitive method (e.g. `String#index`) used to shadow the primitive
# during caller-side type inference. `def find_bracket(s); s.index("[");
# end` walked s's body, found `index` is defined on user-class
# StringIndexer, and committed s's param type to obj_StringIndexer —
# even though the call site passed a string and `index` is a primitive
# String method.
#
# Fix: extend `called_methods_only_on_container_builtins` to also
# accept the methods shared across String / Hash / Integer, so
# `def index` / `def show` / `def create` etc. on user classes can
# coexist with primitive callers.

class StringIndexer
  def index(needle, start = 0)
    "from-StringIndexer"
  end
end

def find_bracket(s)
  s.index("[")
end
puts find_bracket("hello[world]")    # 5

# String method that's also defined on a controller-shaped class.
class ArticlesController
  def show
    "ArticlesController#show"
  end
end

def first_or_default(s)
  s.start_with?("/")    # String#start_with? — should not be shadowed
end
puts first_or_default("/articles")    # true
puts first_or_default("articles")     # false

# Hash-shared `keys` shouldn't pull a user-class param into obj_X.
class KeyHolder
  def keys
    ["alpha", "beta"]
  end
end

def count_keys(h)
  h.keys.length
end
puts count_keys({a: 1, b: 2, c: 3})   # 3
