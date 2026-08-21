# A bare `raise` re-raises the exception its own rescue caught, even when the
# rescue body ran another begin/rescue first. The saved class/message buffers
# were shared across clauses, so the inner clause overwrote the outer one's
# numbering and the re-raise named temporaries that were out of scope by then
# -- the C build failed (#4052).

def inner = raise(ArgumentError, "inner boom")

def cleanup_that_raises
  yield
rescue StandardError
  begin
    inner
  rescue StandardError => e
    puts "swallowed: #{e.class}: #{e.message}"
  end
  raise
end

begin
  cleanup_that_raises { raise "outer boom" }
rescue StandardError => e
  puts "caught: #{e.class}: #{e.message}"
end

# the same with an ensure on the inner handler
def cleanup_with_ensure
  yield
rescue StandardError
  begin
    inner
  rescue StandardError
    nil
  ensure
    puts "ensured"
  end
  raise
end

begin
  cleanup_with_ensure { raise TypeError, "typed boom" }
rescue StandardError => e
  puts "caught: #{e.class}: #{e.message}"
end

# two levels of nesting inside one rescue body
def two_levels
  yield
rescue StandardError
  begin
    begin
      inner
    rescue StandardError => e
      puts "inner: #{e.message}"
    end
    raise "middle boom"
  rescue StandardError => e
    puts "middle: #{e.message}"
  end
  raise
end

begin
  two_levels { raise "outermost boom" }
rescue StandardError => e
  puts "caught: #{e.message}"
end

# a nested rescue in the body, then an explicit raise of a NEW exception
def replaces
  yield
rescue StandardError
  begin
    inner
  rescue StandardError
    nil
  end
  raise RuntimeError, "replacement"
end

begin
  replaces { raise "original" }
rescue StandardError => e
  puts "caught: #{e.message}"
end
