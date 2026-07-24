begin
  abort("bye now")
  puts "unreached"
rescue SystemExit => e
  puts "status=#{e.status}"
  puts "msg=#{e.message}"
end
puts "continued"
begin
  abort
rescue SystemExit => e
  puts "noarg-status=#{e.status}"
  puts "noarg-msg=#{e.message}"
end
