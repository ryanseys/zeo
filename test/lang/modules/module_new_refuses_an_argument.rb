[
  -> { Module.new(1) },
  -> { Module.new(1, 2) },
  -> { Module.new(1) { } },
  -> { Class.new(String, 2) },
  -> { Class.new(String) },
  -> { Module.new },
].each do |f|
  begin
    puts "ok #{f.call.class}"
  rescue ArgumentError => e
    puts "ArgumentError: #{e.message}"
  end
end
__END__
ArgumentError: wrong number of arguments (given 1, expected 0)
ArgumentError: wrong number of arguments (given 2, expected 0)
ArgumentError: wrong number of arguments (given 1, expected 0)
ArgumentError: wrong number of arguments (given 2, expected 0..1)
ok Class
ok Module
