module Store
  class Base
    def base = "base"
  end
  class Item < Base
    autoload :Nested, File.expand_path("aitem", __dir__)
    def own = "own"
  end
end
