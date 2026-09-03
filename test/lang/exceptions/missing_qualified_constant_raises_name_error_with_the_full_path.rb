module Store
end

begin
  puts Store::MISSING
rescue NameError => e
  puts "NameError: #{e.message}"
end
__END__
NameError: uninitialized constant Store::MISSING
