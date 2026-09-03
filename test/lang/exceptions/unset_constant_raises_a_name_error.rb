begin
  puts UNDEFINED_CONST
rescue NameError => e
  puts "caught: #{e.message}"
end
__END__
caught: uninitialized constant UNDEFINED_CONST
