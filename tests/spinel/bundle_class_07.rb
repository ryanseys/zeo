# Bundled tests:
#   - ensure_runs_on_raise_nested
#   - ensure_runs_on_return_from_rescue

# === ensure_runs_on_raise_nested ===
# Nested `begin..ensure..end` with raise — both ensures must run
# (inner first, then outer) before the exception reaches the
# enclosing rescue.

class T_ensure_runs_on_raise_nested_T
  def initialize
    @inner = "no"
    @outer = "no"
    @log = ""
  end

  def f
    begin
      begin
        raise "inner-boom"
      ensure
        @inner = "yes"
        @log = @log + "i"
      end
    ensure
      @outer = "yes"
      @log = @log + "o"
    end
  end

  def report
    begin
      f
    rescue => e
      puts e
    end
    puts @inner
    puts @outer
    puts @log
  end
end

T_ensure_runs_on_raise_nested_T.new.report

# === ensure_runs_on_return_from_rescue ===
# `begin..rescue..ensure..end` — when the rescue body exits via
# `return`, the ensure clause must still run before the function
# returns.

class T_ensure_runs_on_return_from_rescue_C
  def initialize
    @cleanup = "no"
  end

  def f
    begin
      raise "boom"
    rescue
      return 99
    ensure
      @cleanup = "yes"
    end
  end

  def report
    puts f
    puts @cleanup
  end
end

T_ensure_runs_on_return_from_rescue_C.new.report

