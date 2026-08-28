$LOAD_PATH.unshift(File.expand_path("fixtures/raising_feature", __dir__))
here = File.expand_path("fixtures/raising_feature/raises_on_load.rb", __dir__)

begin
  require "raises_on_load"
rescue ArgumentError => e
  puts "require        rescued #{e.class}: #{e.message}"
else
  puts "require        NOT rescued"
end

begin
  require_relative "fixtures/raising_feature/raises_on_load"
rescue ArgumentError => e
  puts "require_relative rescued #{e.class}: #{e.message}"
else
  puts "require_relative NOT rescued"
end

begin
  load here
rescue ArgumentError => e
  puts "load           rescued #{e.class}: #{e.message}"
else
  puts "load           NOT rescued"
end

puts "still running"
