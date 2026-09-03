module Store
  class Item
    def tag
      "tagged"
    end
  end
end

module Store
  class Item
    def more
      "reopened nested"
    end
  end
end

class Store::Item
  def qual
    "qualified reopen"
  end
end

i = Store::Item.new
puts i.tag
puts i.more
puts i.qual
__END__
tagged
reopened nested
qualified reopen
