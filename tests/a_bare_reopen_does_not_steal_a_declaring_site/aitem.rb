# Reopened BARE, in a file the compiler walks BEFORE the one that declares
# the class with its superclass.
module Store
  class Item
    class Nested
      def where = "aitem"
    end
  end
end
