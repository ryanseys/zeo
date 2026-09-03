class MyError < StandardError
  def initialize(msg, code)
    super(msg)
    @code = code
  end
  def code
    @code
  end
end

class Inner
  def call
    begin
      raise MyError.new("failed with code 42", 42)
    rescue MyError => e
      puts "inner saw code #{e.send(:code)}"
      raise
    end
  end
end

begin
  Inner.new.call
rescue MyError => e
  puts "outer saw code #{e.send(:code)}"
  puts "outer message: #{e.send(:message)}"
end
__END__
inner saw code 42
outer saw code 42
outer message: failed with code 42
