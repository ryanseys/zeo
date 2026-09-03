begin
  raise ScriptError, "script problem"
rescue Exception => e
  puts "caught via Exception: #{e.send(:message)}"
end
__END__
caught via Exception: script problem
