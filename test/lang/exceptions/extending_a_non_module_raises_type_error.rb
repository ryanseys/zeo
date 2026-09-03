begin
  Object.new.extend(42)
rescue TypeError => e
  puts "TypeError"
end
__END__
TypeError
