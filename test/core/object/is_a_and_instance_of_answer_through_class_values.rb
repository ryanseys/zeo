class Widget
end
module Helper
end

w = Widget.new
puts w.instance_of?(Widget)
puts w.instance_of?(Object)
puts 5.instance_of?(Integer)
puts Widget.is_a?(Class)
puts Widget.is_a?(Module)
puts Helper.is_a?(Module)
puts Helper.is_a?(Class)
puts Widget.ancestors.first == Widget
puts Widget.ancestors.include?(Object)
__END__
true
false
true
true
true
true
false
true
true
