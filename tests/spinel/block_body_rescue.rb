r001 = [1, 2].map do |i|
  raise ArgumentError, "x" if i == 1
  i
rescue ArgumentError
  :handled
end
p r001
r2 = [1, 2].map { |i| begin; raise ArgumentError if i == 1; i; rescue ArgumentError; :h; end }
p r2
r3 = [1,2,3].each do |i|
  next if i == 2
rescue StandardError
  :no
end
p r3
def m3; yield 1; rescue; :caught; end
p(m3 { |x| x * 2 })
