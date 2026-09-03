# `Array#bsearch`'s block must answer numeric, true, false or nil; any
# other type is a TypeError naming the class. zeo treats a String result as
# a truthy find-minimum hit and answers an element. (Found by the
# 2026-08-24 probe sweep.)
begin
  p [1, 2].bsearch { |x| "s" }
rescue TypeError => e
  puts e.message
end
__END__
wrong argument type String (must be numeric, true, false or nil)
