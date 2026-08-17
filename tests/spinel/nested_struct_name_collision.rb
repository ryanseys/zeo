# Two sibling namespaces each defining `TreeNode` -- one a class, one a Struct
# constant. Only `class` definitions were qualified by their enclosing path, so
# the Struct constant kept the bare leaf name and every reference to it bound
# to the other namespace's class (a three-member construction reached a
# two-parameter constructor and raised ArgumentError).

module Trees
  class Obj
    class TreeNode
      attr_accessor :item, :left
      def initialize(item, depth = 0)
        @item = item
        @left = depth
      end
    end

    def run
      n = TreeNode.new(5, 2)
      n.item + n.left
    end
  end

  class Arena
    TreeNode = Struct.new(:item, :left, :right)

    def run
      n = TreeNode.new(7, -1, -1)
      n.item + n.left + n.right
    end
  end
end

p Trees::Obj.new.run
p Trees::Arena.new.run
