begin
  begin
    raise "boom"
  ensure
    puts "ensure ran"
  end
rescue => e
  puts "caught outside: #{e.send(:message)}"
end
__END__
ensure ran
caught outside: boom
