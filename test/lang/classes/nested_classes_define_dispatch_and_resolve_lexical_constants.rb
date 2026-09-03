module Store
  DEFAULT = 10

  class Item
    def price
      DEFAULT
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

puts Store::Item.new.price
puts Store::Errors::NotFound.new.msg
puts Store::DEFAULT
__END__
10
not found
10
