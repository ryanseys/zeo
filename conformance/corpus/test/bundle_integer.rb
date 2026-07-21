# Bundled tests:
#   - integer_bit_index
#   - integer_bits
#   - integer_ceildiv
#   - integer_divmod
#   - integer_lcm
#   - integer_powmod
#   - integer_pred
#   - integer_sqrt

# === integer_bit_index ===
def t_integer_bit_index
  # `Integer#[N]` returns bit N (0-indexed from the LSB) of the
  # integer. `data[5]` was previously falling through to the
  # unknown-method 0-fallback. Lower as `((rc >> idx) & 1)`.
  
  # Static index
  n = 0b10110100
  puts n[0]    # 0
  puts n[1]    # 0
  puts n[2]    # 1
  puts n[3]    # 0
  puts n[4]    # 1
  puts n[5]    # 1
  puts n[6]    # 0
  puts n[7]    # 1
  
  # Dynamic index
  i = 0
  while i < 4
    puts n[i]
    i += 1
  end
  # 0 0 1 0
end
t_integer_bit_index

# === integer_bits ===
def t_integer_bits
  # Integer bit-test predicates: allbits? / nobits? / anybits?. Three
  # parallel methods (true iff the bits in the mask are: all set / all
  # clear / any set on the receiver). Same coverage matrix per method:
  # basic, no-overlap / subset, zero mask, single bit, large value,
  # negative receiver. Was three near-identical files.
  
  # === allbits? ===
  puts 255.allbits?(255)
  puts 255.allbits?(128)
  puts 0.allbits?(0)
  puts 42.allbits?(0)
  puts 5.allbits?(6)
  puts 8.allbits?(3)
  puts 4.allbits?(4)
  puts 4.allbits?(2)
  puts 0xFFFF.allbits?(0xFF00)
  puts((-1).allbits?(255))
  puts 0.allbits?(1)
  
  # === nobits? ===
  puts 256.nobits?(1)
  puts 256.nobits?(255)
  puts 8.nobits?(4)
  puts 255.nobits?(1)
  puts 6.nobits?(2)
  puts 0.nobits?(0)
  puts 42.nobits?(0)
  puts 4.nobits?(2)
  puts 4.nobits?(4)
  puts((-1).nobits?(1))
  puts((-4).nobits?(2))
  puts 0xFF00.nobits?(0x00FF)
  
  # === anybits? ===
  puts 255.anybits?(128)
  puts 255.anybits?(1)
  puts 0.anybits?(1)
  puts 16.anybits?(8)
  puts 4.anybits?(2)
  puts 0.anybits?(0)
  puts 42.anybits?(0)
  puts 6.anybits?(4)
  puts 6.anybits?(2)
  puts((-1).anybits?(1))
  puts((-4).anybits?(4))
  puts 0xFF00.anybits?(0x0100)
end
t_integer_bits

# === integer_ceildiv ===
def t_integer_ceildiv
  # basic
  puts 7.ceildiv(2)
  puts 10.ceildiv(5)
  puts 11.ceildiv(5)
  
  # exact division
  puts 6.ceildiv(3)
  
  # zero dividend
  puts 0.ceildiv(3)
  
  # one
  puts 1.ceildiv(1)
  
  # negative
  puts((-7).ceildiv(2))
  puts 7.ceildiv(-2)
  puts((-7).ceildiv(-2))
  
  # large
  puts 1000001.ceildiv(1000)
end
t_integer_ceildiv

# === integer_divmod ===
def t_integer_divmod
  p 100.divmod(7)
  # Float#divmod returns [Integer, Float] — exercises the
  # tuple:int,float inspect arm.
  p 5.5.divmod(2)
end
t_integer_divmod

# === integer_lcm ===
def t_integer_lcm
  # basic
  puts 6.lcm(4)
  puts 4.lcm(6)
  
  # zero
  puts 0.lcm(5)
  puts 5.lcm(0)
  puts 0.lcm(0)
  
  # negative
  puts((-4).lcm(6))
  puts 6.lcm(-4)
  puts((-3).lcm(-7))
  
  # same
  puts 7.lcm(7)
  
  # one
  puts 1.lcm(5)
  puts 5.lcm(1)
  
  # coprime
  puts 8.lcm(9)
  
  # divisor — one divides the other
  puts 3.lcm(9)
  puts 9.lcm(3)
  
  # primes
  puts 7.lcm(13)
  
  # one with one
  puts 1.lcm(1)
  
  # large
  puts 12345.lcm(67890)
end
t_integer_lcm

# === integer_powmod ===
def t_integer_powmod
  # basic
  puts 2.pow(10, 1000)
  puts 5.pow(2, 3)
  puts 3.pow(3, 8)
  
  # exp zero
  puts 2.pow(0, 5)
  puts 0.pow(0, 5)
  
  # mod one — always zero
  puts 2.pow(100, 1)
  puts 999.pow(999, 1)
  
  # negative base
  puts((-2).pow(3, 5))
  puts((-3).pow(2, 7))
  
  # large exponent
  puts 2.pow(20, 1000000)
  puts 7.pow(15, 100)
  
  # base larger than mod
  puts 100.pow(3, 7)
  
  # base zero
  puts 0.pow(5, 3)
  
  # base one
  puts 1.pow(999, 7)
  
  # mod two
  puts 7.pow(3, 2)
  
  # exp one
  puts 5.pow(1, 3)
  
  # negative mod
  puts 2.pow(2, -3)
  puts 2.pow(3, -5)
  
  # one-arg pow
  puts 2.pow(10)
  puts 3.pow(3)
end
t_integer_powmod

# === integer_pred ===
def t_integer_pred
  # basic
  puts 5.pred
  puts 1.pred
  puts 0.pred
  
  # negative
  puts((-1).pred)
  puts((-100).pred)
  
  # large
  puts 1000000.pred
end
t_integer_pred

# === integer_sqrt ===
def t_integer_sqrt
  p Integer.sqrt(0)
  p Integer.sqrt(1)
  p Integer.sqrt(16)
  p Integer.sqrt(99)
  p Integer.sqrt(100)
  p Integer.sqrt(101)
  p Integer.sqrt(1_000_000)
  
  # Large-integer precision — beyond the 53-bit double mantissa.
  # A double-based sqrt would round and produce off-by-one results
  # for values above ~2^53; the Newton-method helper stays exact.
  p Integer.sqrt(2**53)
  p Integer.sqrt(2**53 + 1)
  p Integer.sqrt(2**60)
  p Integer.sqrt(2**62 - 1)
end
t_integer_sqrt

