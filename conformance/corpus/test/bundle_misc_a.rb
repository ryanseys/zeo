# Bundled tests:
#   - catch
#   - class_threeq
#   - do_while_post_test_loop
#   - fiber
#   - float
#   - math
#   - misc2
#   - numeric
#   - safe_nav
#   - shuffle

# === catch ===
def t_catch
  # Test catch/throw
  
  result = catch(:done) do
    throw :done, 42
    999  # unreachable
  end
  puts result  # 42
  
  # catch without throw returns block value
  result2 = catch(:nope) do
    100
  end
  puts result2  # 100
  
  # Nested catch (same type)
  result3 = catch(:outer) do
    catch(:inner) do
      throw :outer, 77
    end
    0
  end
  puts result3  # 77
  
  puts "done"
end
t_catch

# === class_threeq ===
def t_class_threeq
  # Class#=== — case-when membership for primitive type names.
  # Compile-time decided based on the arg's inferred type.
  p Integer === 5
  p Integer === 5.0
  p Integer === "x"
  p Numeric === 5
  p Numeric === 5.0
  p Comparable === 5
  p Float === 5.0
  p Float === 5
  p String === "hi"
  p String === :hi
  p Symbol === :hi
  p Symbol === "hi"
  p Array === [1, 2, 3]
  p Array === "x"
  p Range === (1..3)
  p Range === 5
  p TrueClass === true
  p FalseClass === false
  p NilClass === nil
  p Object === 5
  p Object === "x"
  
  # More true/false coverage for the remaining primitive arms.
  p Hash === {a: 1}
  p Hash === [1, 2, 3]
  p Numeric === "5"
  p Numeric === :sym
  p Comparable === 1.5
  p Comparable === [1, 2]
  p TrueClass === false
  p FalseClass === true
  p NilClass === 0
  p NilClass === false
  
  # Kernel and BasicObject — Ruby's universal ancestors. Every receiver
  # is in the Object hierarchy, so `=== anything` is true.
  p Kernel === 5
  p Kernel === "x"
  p Kernel === nil
  p BasicObject === 5
  p BasicObject === [1]
end
t_class_threeq

# === do_while_post_test_loop ===
def t_do_while_post_test_loop
  # Prism's PM_LOOP_FLAGS_BEGIN_MODIFIER (= 4, bit 2) marks
  # `begin..end while cond` / `begin..end until cond` as post-test
  # loops — body runs at least once. Spinel was treating them as
  # plain pre-test `while`, so a body that should have run once but
  # whose condition was false on entry never ran at all.
  
  ran = 0
  begin
    ran += 1
  end while ran > 99      # cond false on entry → post-test runs body once
  puts ran                # 1
  
  ran2 = 0
  begin
    ran2 += 1
  end until ran2 < 99     # cond true on entry → post-test runs body once
  puts ran2               # 1
  
  # Sanity: bare pre-test `while` with cond false on entry runs zero
  # times.
  ran3 = 0
  while ran3 > 99 do ran3 += 1 end
  puts ran3               # 0
end
t_do_while_post_test_loop

# === fiber ===
def t_fiber
  # Test Fiber (cooperative concurrency)
  
  # Basic yield/resume
  f = Fiber.new {
    Fiber.yield(10)
    Fiber.yield(20)
    30
  }
  puts f.resume  # 10
  puts f.resume  # 20
  puts f.resume  # 30
  
  # Value passing
  f2 = Fiber.new { |first|
    second = Fiber.yield(first * 2)
    second * 3
  }
  puts f2.resume(5)   # 10
  puts f2.resume(7)   # 21
  
  # String passing
  f3 = Fiber.new {
    Fiber.yield("hello")
    Fiber.yield("world")
    "done"
  }
  puts f3.resume  # hello
  puts f3.resume  # world
  puts f3.resume  # done
  
  # alive?
  f4 = Fiber.new {
    Fiber.yield(1)
    2
  }
  puts f4.alive?   # true
  f4.resume
  puts f4.alive?   # true
  f4.resume
  puts f4.alive?   # false
  
  # Fiber.current
  cur = Fiber.current
  puts cur.alive?  # true
  
  # FiberError on dead fiber
  f5 = Fiber.new { 42 }
  f5.resume
  begin
    f5.resume
    puts "ERROR"
  rescue FiberError
    puts "caught FiberError"
  end
end
t_fiber

# === float ===
def t_float
  # Float and FloatArray coverage — to_s formatting, FloatArray
  # reductions / slicing / shift, and Float#ceil/floor/round/truncate
  # with a precision arg. Was five separate tests; merged. No class
  # collisions; locals reused across the originals (`f`, `arr`, `a`)
  # get per-section prefixes so spinel's local-type inference doesn't
  # unify them.
  
  # === FloatArray reductions: min / max / sum / first / last ===
  fr_arr = [1.5, 2.5, 0.5, 3.5]
  puts fr_arr.min     # 0.5
  puts fr_arr.max     # 3.5
  puts fr_arr.first   # 1.5
  puts fr_arr.last    # 3.5
  
  fr_neg = [-1.5, -3.25, 2.75]
  puts fr_neg.min     # -3.25
  puts fr_neg.max     # 2.75
  
  fr_one = [4.5]
  puts fr_one.min     # 4.5
  puts fr_one.max     # 4.5
  
  # Sum with non-integer-valued result (avoids the stale-type truncation
  # that would print "4" instead of "4.5").
  fr_sum = [1.5, 2.5, 0.5]
  puts fr_sum.sum     # 4.5
  
  # === FloatArray slicing: a[range] and a[start, len] ===
  fs_a = [1.5, 2.5, 3.5, 4.5, 5.5]
  fs_b = fs_a[1..3]
  puts fs_b.length    # 3
  puts fs_b[0]        # 2.5
  puts fs_b[1]        # 3.5
  puts fs_b[2]        # 4.5
  fs_c = fs_a[1, 2]
  puts fs_c.length    # 2
  puts fs_c[0]        # 2.5
  puts fs_c[1]        # 3.5
  fs_d = fs_a[-2, 2]
  puts fs_d.length    # 2
  puts fs_d[0]        # 4.5
  puts fs_d[1]        # 5.5
  fs_e = fs_a[2, 100]
  puts fs_e.length    # 3 (clamped)
  puts fs_e[0]        # 3.5
  puts fs_e[2]        # 5.5
  puts fs_a[0]        # 1.5
  puts fs_a[-1]       # 5.5
  puts fs_a[1..3].sum # 10.5
  
  # === FloatArray#shift ===
  fsh_arr = [1.5, 2.5, 3.5, 4.5]
  puts fsh_arr.shift  # 1.5
  puts fsh_arr.length # 3
  puts fsh_arr[0]     # 2.5
  while fsh_arr.length > 0
    puts fsh_arr.shift
  end
  puts fsh_arr.length # 0
  
  # === Float#ceil/floor/round/truncate with precision arg ===
  puts 3.14159.round(2)
  puts 3.14159.round(4)
  puts 1.5.round(1)
  puts 2.5.round(1)
  puts 3.14159.ceil(2)
  puts 3.14159.ceil(4)
  puts 1.001.ceil(2)
  puts 3.14159.floor(2)
  puts 3.14159.floor(4)
  puts 1.999.floor(2)
  puts 3.14159.truncate(2)
  puts 3.14159.truncate(4)
  puts (-1.567).truncate(2)
  puts 3.14.round
  puts 3.14.ceil
  puts 3.14.floor
  puts 3.14.truncate
  # Negative precision: bool-compare for type-stable output across
  # CRuby's Integer-return rule vs. Spinel's uniform Float inference.
  puts 12345.6789.floor(-2) == 12300
  puts 12345.6789.ceil(-2) == 12400
  puts 12345.6789.round(-1) == 12350
  puts 12345.6789.truncate(-2) == 12300
  puts (-12345.6789).floor(-2) == -12400
  puts (-12345.6789).ceil(-2) == -12300
  
  # === Float#to_s / p / puts byte-identical output ===
  # Shortest decimal that round-trips; fixed-point inside CRuby's
  # [-4, 15] decimal-exponent window, scientific (`d.ddde+NN`)
  # outside.
  puts 1.0
  puts 100.0
  puts(-3.25)
  puts 1234567890.0
  puts 1234567890.5
  puts 0.1
  puts 0.3
  puts 0.30000000000000004
  puts 0.0001
  puts 0.00001
  puts 1.5e14
  puts 1.0e15
  puts 9.99e15
  puts 1.0e16
  puts 1.0e100
  puts(-0.0)
  puts Float::INFINITY
  puts(-Float::INFINITY)
  puts Float::NAN
  p 1.0
  p 1234567890.5
  p 1.0e16
  p(-0.0)
  
  # === Kernel#Float coerces strings, ints, and floats ===
  puts Float("3.14")        # 3.14
  puts Float("0")           # 0.0
  puts Float("-2.5")        # -2.5
  puts Float(1)             # 1.0
  puts Float(42)            # 42.0
  puts Float(2.71)          # 2.71
  puts Float("1e2")         # 100.0
end
t_float

# === math ===
def t_math
  # Math module methods. All should return Float.
  
  # Trig (in radians)
  puts (Math.sin(0.0) * 1000).to_i        # 0
  puts (Math.cos(0.0) * 1000).to_i        # 1000
  puts (Math.tan(0.0) * 1000).to_i        # 0
  puts (Math.sin(0.5) * 1000).to_i        # 479
  puts (Math.cos(0.5) * 1000).to_i        # 877
  puts (Math.tan(0.5) * 1000).to_i        # 546
  
  # Inverse trig
  puts (Math.atan(1.0) * 1000).to_i       # 785
  puts (Math.atan2(1.0, 1.0) * 1000).to_i # 785
  puts (Math.asin(1.0) * 1000).to_i       # 1570
  puts (Math.acos(0.0) * 1000).to_i       # 1570
  
  # Powers / logs
  puts (Math.sqrt(2.0) * 1000).to_i       # 1414
  puts (Math.log(1.0) * 1000).to_i        # 0
  puts (Math.log2(8.0) * 1000).to_i       # 3000
  puts (Math.log10(100.0) * 1000).to_i    # 2000
  puts (Math.exp(0.0) * 1000).to_i        # 1000
  
  # Float-typed result (would print "1" / "0" if inferred as int).
  # Use the *1000+to_i idiom to dodge precision-formatting differences
  # between Spinel's float-puts and CRuby's.
  puts (Math.log2(3.0) * 1000).to_i       # 1584
  puts (Math.log10(3.0) * 1000).to_i      # 477
  
  # Hypot
  puts (Math.hypot(3.0, 4.0) * 1000).to_i # 5000
  
  # Hyperbolic
  puts (Math.tanh(0.5) * 1000).to_i       # 462
  puts (Math.tanh(1.0) * 1000).to_i       # 761
  puts (Math.sinh(1.0) * 1000).to_i       # 1175
  puts (Math.cosh(1.0) * 1000).to_i       # 1543
  
  # Inverse hyperbolic
  puts (Math.atanh(0.5) * 1000).to_i      # 549
  puts (Math.asinh(1.0) * 1000).to_i      # 881
  puts (Math.acosh(2.0) * 1000).to_i      # 1316
  
  # PI
  puts (Math::PI * 1000).to_i             # 3141
end
t_math

# === misc2 ===
def t_misc2
  # Test format/sprintf
  puts format("%d:%02d", 5, 3)
  puts sprintf("hello %s %d", "world", 42)
  
  # Test inline rescue as expression
  x = raise("oops") rescue 42
  puts x
  
  # Test symbol key hash (string values -> sp_RbHash)
  h = {running: "green", waiting: "yellow"}
  puts h[:running]
  puts h[:waiting]
  
  # Test symbol key hash with integer values (-> sp_StrIntHash)
  h2 = {a: 1, b: 2}
  puts h2[:a]
  puts h2[:b]
end
t_misc2

# === numeric ===
def t_numeric
  # Test numeric methods and conversions
  
  puts (-5).abs     # 5
  puts 3.14.to_i    # 3
  puts 3.14.ceil    # 4
  puts 3.14.floor   # 3
  puts 3.14.round   # 3
  puts 3.75.round   # 4
  
  # Integer methods
  puts 7.even?      # false
  puts 8.even?      # true
  puts 7.odd?       # true
  puts 10.zero?     # false
  puts 0.zero?      # true
  
  # Numeric abs
  puts (-3.14).abs  # 3.14
  
  # Power
  puts 2 ** 10      # 1024
end
t_numeric

# === safe_nav ===
def t_safe_nav
  # Test safe navigation operator &.
  
  s = "hello"
  puts s&.length    # 5
  puts s&.upcase    # HELLO
  
  arr = Array.new
  arr.push(10)
  arr.push(20)
  puts arr&.length  # 2
  
  # Chain
  puts "world"&.upcase&.length  # 5
  
  puts "done"
end
t_safe_nav

# === shuffle ===
def t_shuffle
  arr = [1, 2, 3, 4, 5]
  s = arr.shuffle
  puts s.length
  puts s.sort.join(",")
  puts arr.join(",")
  
  words = ["foo", "bar", "baz", "qux"]
  s2 = words.shuffle
  puts s2.length
  puts s2.include?("foo")
  puts s2.include?("bar")
  puts s2.include?("baz")
  puts s2.include?("qux")
  puts words.join(",")
  
  nums = [10, 20, 30]
  nums.shuffle!
  puts nums.length
  puts nums.sort.join(",")
  
  # FloatArray
  floats = [1.5, 2.5, 3.5, 4.5]
  fs = floats.shuffle
  puts fs.length            # 4
  puts floats.length        # 4 (original unchanged)
  floats.shuffle!
  puts floats.length        # 4 (in-place, length stable)
  
  # Empty / single-element edge cases stay stable.
  empty = []
  empty.shuffle!
  puts empty.length         # 0
  one = [42]
  one.shuffle!
  puts one[0]               # 42
end
t_shuffle

