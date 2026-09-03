begin
  eval(123)
rescue TypeError => e
  puts e.message
end
__END__
no implicit conversion of Integer into String
