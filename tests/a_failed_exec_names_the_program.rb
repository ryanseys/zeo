begin
  exec("/no/such/program-zeo")
rescue SystemCallError => e
  puts "#{e.class}: #{e.message}"
end
