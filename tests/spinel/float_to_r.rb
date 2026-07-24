# Float#to_r (exact) and Float#rationalize(eps) / rationalize.
# Route the receiver through a param to exercise the runtime path, not a fold.
def id(x) = x

# --- to_r: exact rational value of the double ---
p id(1.5).to_r            # (3/2)
p id(0.75).to_r           # (3/4)
p id(2.0).to_r            # (2/1)
p id(-1.5).to_r           # (-3/2)
p id(0.0).to_r            # (0/1)
p id(0.25).to_r           # (1/4)
p id(-0.125).to_r         # (-1/8)

# --- rationalize(eps): simplest fraction within eps ---
p id(0.3).rationalize(0.01)     # (3/10)
p id(2.5).rationalize(0.1)      # (5/2)
p id(0.333333).rationalize(0.001) # (1/3)
p id(-0.3).rationalize(0.01)    # (-3/10)
p id(3.14159).rationalize(0.001) # simplest within 0.001

# --- rationalize (no arg): simplest fraction that round-trips ---
p id(1.5).rationalize     # (3/2)
p id(0.3).rationalize     # (3/10)

# --- the result is a Rational and composes with Rational arithmetic ---
p (id(1.5).to_r + id(0.5).to_r)   # (2/1)
p id(1.5).to_r.numerator          # 3
p id(1.5).to_r.denominator        # 2
p id(0.3).rationalize(0.01).to_f  # 0.3

# --- literal receivers fold but must agree too ---
p 1.5.to_r
p 0.3.rationalize(0.01)
