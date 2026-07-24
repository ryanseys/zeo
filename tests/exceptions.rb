begin
  puts "try"
  raise ArgumentError, "bad"
rescue ArgumentError => e
  puts "caught: #{e.send(:message)}"
ensure
  puts "ensure ran"
end

begin
  puts "body"
rescue => e
  puts "rescued"
else
  puts "else ran"
ensure
  puts "ensure2 ran"
end

class AppError < StandardError
end
class ValidationError < AppError
end

begin
  raise ValidationError, "bad input"
rescue AppError => e
  puts "caught as AppError: #{e.send(:message)}"
end

begin
  raise TypeError, "wrong type"
rescue ArgumentError, TypeError => e
  puts "caught one of: #{e.send(:message)}"
end

class Attempt
  def initialize
    @attempts = 0
  end

  def attempts
    @attempts
  end

  def run
    begin
      @attempts += 1
      raise "fail" if @attempts < 3
      puts "succeeded after #{@attempts} attempts"
    rescue
      retry if @attempts < 3
    ensure
      puts "ensure ran, attempts=#{@attempts}"
    end
  end
end
Attempt.new.run

class Worker
  def safe
    raise "oops"
  rescue => e
    "handled: #{e.send(:message)}"
  end
end
puts Worker.new.safe

class ReturnTester
  def m
    begin
      raise "x"
    rescue
      return "returned"
    ensure
      puts "ensure ran before return"
    end
  end
end
puts ReturnTester.new.m

class Unsafe
  def op(n)
    raise "negative" if n < 0
    n * 2
  end
end
class Calc
  def safe_op(n) = Unsafe.new.op(n) rescue -1
end
c = Calc.new
puts c.safe_op(5)
puts c.safe_op(-5)

class TopRisk
  def risky_top
    raise "bad"
  end
end
x = TopRisk.new.risky_top rescue "fallback"
puts x

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
