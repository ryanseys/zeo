module Store
  DEFAULT = 10

  class Item
    def initialize(name)
      @name = name
    end

    def price
      DEFAULT
    end

    def label
      "#{@name} ($#{price})"
    end
  end

  class Errors
    class NotFound
      def msg
        "not found"
      end
    end
  end
end

module Registry
  class Item
    def price
      99
    end
  end
end

puts Store::Item.new("apple").label
puts Registry::Item.new.price
puts Store::Errors::NotFound.new.msg
puts Store::DEFAULT

class Store::Cart
  def initialize
    @items = []
  end

  def add(item)
    @items << item
    self
  end

  def count
    @items.length
  end
end

cart = Store::Cart.new
cart.add(Store::Item.new("pear")).add(Store::Item.new("plum"))
puts cart.count
