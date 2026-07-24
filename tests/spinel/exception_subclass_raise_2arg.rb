class RecordInvalid < StandardError
  attr_reader :record
  def initialize(record)
    super("validation failed")
    @record = record
  end
end
begin
  raise RecordInvalid, "the-rec"
rescue RecordInvalid => e
  puts e.message
  puts e.record
end
