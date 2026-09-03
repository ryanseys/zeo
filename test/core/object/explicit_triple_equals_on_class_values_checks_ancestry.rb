class Widget
end

puts Integer === 5
puts Widget === Widget.new
puts Widget === 5
__END__
true
true
false
