# `Errno::ENOENT` from a failed `exec` carries the program it could not
# find. zeo raises the right class with an EMPTY subject, so the message
# ends at the dash and says nothing about what was missing.
#
# Carved out of tests/probe_messages.rb ("exec missing").
begin
  exec("/no/such/program-zeo")
rescue SystemCallError => e
  puts "#{e.class}: #{e.message}"
end
