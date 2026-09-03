# clamp must raise ArgumentError when the minimum bound exceeds the maximum,
# and Float#clamp must also raise when any operand is NaN (comparisons with
# NaN are undefined). Receiver and bounds are routed through method params so
# the runtime helpers are exercised, not a compile-time fold.
def show_i(label, x, lo, hi)
  print label, ": "
  p x.clamp(lo, hi)
rescue => e
  puts "#{e.class}: #{e.message}"
end

def show_ir(label, x)
  print label, ": "
  p x.clamp(10..2)
rescue => e
  puts "#{e.class}: #{e.message}"
end

def show_f(label, x, lo, hi)
  print label, ": "
  p x.clamp(lo, hi)
rescue => e
  puts "#{e.class}: #{e.message}"
end

nan = 0.0 / 0.0

show_i("int ok", 5, 1, 9)
show_i("int min>max", 5, 10, 2)
show_ir("int range bad", 5)
show_f("float ok", 5.0, 1.5, 3.5)
show_f("float min>max", 5.0, 2.0, 1.0)
show_f("float recv nan", nan, 1.0, 2.0)
show_f("float lo nan", 1.5, nan, 2.0)
show_f("float hi nan", 1.5, 1.0, nan)
__END__
int ok: 5
int min>max: ArgumentError: min argument must be less than or equal to max argument
int range bad: ArgumentError: min argument must be less than or equal to max argument
float ok: 3.5
float min>max: ArgumentError: min argument must be less than or equal to max argument
float recv nan: ArgumentError: comparison of Float with 1.0 failed
float lo nan: ArgumentError: comparison of Float with 2.0 failed
float hi nan: ArgumentError: comparison of Float with NaN failed
