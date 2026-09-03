Vec = Data.define(:x, :y)

top = [Vec.new(3, 4)].first
case top
in { x:, y: }
  puts "#{x},#{y}"
end
__END__
3,4
