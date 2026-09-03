begin
  begin
    raise TypeError, "inner"
  rescue ArgumentError
    puts "wrong handler"
  end
rescue TypeError => e
  puts "outer caught: #{e.send(:message)}"
end
__END__
outer caught: inner
