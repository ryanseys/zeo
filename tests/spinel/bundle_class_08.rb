# Bundled tests:
#   - ensure_runs_on_return_nested
#   - enumerable
#   - exceptions

# === ensure_runs_on_return_nested ===
# Nested `begin..ensure..end` — a `return` inside the inner begin
# must replay the inner ensure first, then the outer ensure, then
# return. Both writebacks must take effect, in order.

class T_ensure_runs_on_return_nested_T
  def initialize
    @inner = 0
    @outer = 0
    @log = ""
  end

  def run
    begin
      a = 1
      begin
        b = 10
        a = 2
        return
      ensure
        @inner = b
        @log = @log + "i"
      end
    ensure
      @outer = a
      @log = @log + "o"
    end
  end

  def report
    run
    puts "inner=" + @inner.to_s
    puts "outer=" + @outer.to_s
    puts "log=" + @log
  end
end

T_ensure_runs_on_return_nested_T.new.report

# === enumerable ===
class T_enumerable_NumberList
  include Enumerable
  def initialize
    @data = (1..5).to_a
  end
  def each
    @data.each do |x|
      yield x
    end
  end
end

list = T_enumerable_NumberList.new
total = 0
list.each do |x|
  total += x
end
puts total  # 15
puts "done"

# === exceptions ===
# Test exception classes

class T_exceptions_AppError < RuntimeError
end

class T_exceptions_NotFoundError < T_exceptions_AppError
end

# Raise custom exception
begin
  raise T_exceptions_NotFoundError, "item not found"
rescue T_exceptions_NotFoundError => e
  puts e  # item not found
rescue T_exceptions_AppError => e
  puts "app error"
end

# Raise with class name
begin
  raise T_exceptions_AppError, "something went wrong"
rescue T_exceptions_NotFoundError => e
  puts "not found"
rescue T_exceptions_AppError => e
  puts e  # something went wrong
end

# Bare rescue catches all
begin
  raise "generic error"
rescue => e
  puts e  # generic error
end

puts "done"

