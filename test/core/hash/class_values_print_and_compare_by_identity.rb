class Widget
end

module Store
  class Item
  end
end

puts Widget
p Widget
puts Store::Item
puts Widget == Widget
puts Widget == Store::Item
puts Widget != Store::Item
arr = [Widget, Store::Item]
puts arr.include?(Widget)
__END__
Widget
Widget
Store::Item
true
false
true
true
