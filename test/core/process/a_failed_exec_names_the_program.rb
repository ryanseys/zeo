begin
  exec("/no/such/program-zeo")
rescue SystemCallError => e
  puts "#{e.class}: #{e.message}"
end
__END__
Errno::ENOENT: No such file or directory - /no/such/program-zeo
