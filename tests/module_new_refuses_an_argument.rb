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
