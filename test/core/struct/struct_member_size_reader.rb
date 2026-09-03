# A Struct member named `size` (or `length`) must dispatch to the generated
# member reader, not the built-in length/size, even on a poly receiver.
E = Struct.new(:offset, :size, :name)
entries = 3.times.map do |i|
  E.new(i * 10, i * 100 + 5, "n#{i}")
end
entries.each { |e| puts "#{e.offset},#{e.size},#{e.name}" }
# poly-typed element access
list = entries
puts list[1].size
puts list.map(&:size).inspect
__END__
0,5,n0
10,105,n1
20,205,n2
105
[5, 105, 205]
