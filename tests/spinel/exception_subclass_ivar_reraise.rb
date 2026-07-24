class PlainErr < StandardError; end
class DataErr < StandardError
  attr_reader :data
  def initialize(d); super("data fail"); @data = d; end
end
begin
  raise PlainErr, "plain"
rescue PlainErr => pe
  puts pe.message
end
begin
  raise DataErr.new(42)
rescue StandardError => se   # broader catch: message works
  puts se.message
end
begin
  begin
    raise DataErr.new(7)
  rescue DataErr => de
    puts de.data
    raise
  end
rescue => re
  puts "reraised: #{re.message}"
end
begin
  raise ArgumentError, "bad arg"
rescue ArgumentError => ae
  puts ae.message
end
