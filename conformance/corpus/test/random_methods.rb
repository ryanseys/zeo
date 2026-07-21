# Random / Kernel#rand semantics (#2517-2525, #2543, #2544)
srand(5)
p(srand(10))                 # 2517: returns previous seed (5)
p(rand(0).class)             # 2518: rand(0) -> Float
p(rand(-3).class)            # 2518: rand(neg) -> Integer
p(rand(-3) >= 0 && rand(-3) < 3)
p(rand(5...5))               # 2519: empty range -> nil
p(rand(5..3))                # 2519: reversed range -> nil
r = Random.new(42)
p(r.rand(1.0..2.0).class)    # 2521: Float range -> Float
p(r.seed)                    # 2522: 42
p(Random.new_seed.class)     # 2523: Integer
p(r.class)                   # 2524: Random
p(r == r)                    # 2524: true
p(r.equal?(r))               # 2524
p(Random.new(1) == Random.new(1))  # distinct objects -> false
Random.srand(7)
p(rand(100).class)           # 2525: Integer
p(Random.urandom(8).bytesize) # 2543: 8 bytes. NOT .length: spinel counts
                              # UTF-8 chars, so random bytes that happen to
                              # form multi-byte sequences made it flaky
p(Random.urandom(8).class)
begin; rand(1..); rescue => e; p e.class; end   # 2544: Errno::EDOM
begin; rand(..5); rescue => e; p e.class; end
