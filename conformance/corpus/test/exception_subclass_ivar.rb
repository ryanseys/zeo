class MyError < StandardError
  attr_reader :record
  def initialize(record)
    super("boom")
    @record = record
  end
end
begin
  raise MyError.new("the-record")
rescue MyError => e
  puts e.message
  puts e.record
end
