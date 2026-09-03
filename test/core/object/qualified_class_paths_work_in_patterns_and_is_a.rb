module Store
  class Item
    def initialize(n)
      @n = n
    end

    def deconstruct_keys(keys)
      { n: @n }
    end
  end
end

case Store::Item.new(5)
in Store::Item
  puts "matched class pattern"
end

case Store::Item.new(7)
in Store::Item(n:)
  puts "matched with capture #{n}"
end

puts Store::Item.new(1).is_a?(Store::Item)
__END__
matched class pattern
matched with capture 7
true
