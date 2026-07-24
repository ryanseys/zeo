# Invalid operations reached through boxed values must raise instead of
# silently producing Integer zero.
receivers = [nil, true, :token, {"a" => 1}]

receivers.each do |value|
  begin
    value + 1
  rescue StandardError => error
    p error.class
  end
  begin
    value - 1
  rescue StandardError => error
    p error.class
  end
  begin
    value * 1
  rescue StandardError => error
    p error.class
  end
end

# Arithmetic-capable receivers reject incompatible operands with TypeError.
[1, 1.5].each do |value|
  begin
    value + "x"
  rescue StandardError => error
    p error.class
  end
end

# This is the minimized differential-fuzzer case: the fresh block-local starts
# as nil, so subtraction raises and the surrounding rescue assigns -1.
begin
  [0, 0, 0].each do |element|
    if element > 0
      accumulator = accumulator + element
    else
      accumulator = accumulator - 0
    end
  end
rescue StandardError
  accumulator = -1
end
p accumulator
