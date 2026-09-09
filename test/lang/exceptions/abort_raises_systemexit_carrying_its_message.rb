# Kernel#abort raises SystemExit with status 1 and the message it was given, and the line after it never runs.
# (spinel issue #3077)
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
__END__
status=1
msg=bye now
continued
noarg-status=1
noarg-msg=exit
#@ stderr
bye now
