begin
  begin
    raise "original"
  ensure
    raise "from ensure"
  end
rescue => e
  puts "caught: #{e.send(:message)}"
end
__END__
caught: from ensure
