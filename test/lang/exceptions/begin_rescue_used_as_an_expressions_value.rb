x = begin
  raise "bad"
rescue
  -1
end
puts x
__END__
-1
