# GAP -- imported from the spinel corpus at c55d9bdb.
# An Integer operation with a nil argument PANICS the runtime
# (crates/zeo-rt/src/builtins/integer.rs) instead of raising TypeError.
#
r001 = (1.5.coerce(nil) rescue $!.class)
p r001

r002 = (5.pow(nil) rescue $!.class); p r002        # Spinel: 1
r003 = (5.fdiv(nil) rescue $!.class); p r003       # Spinel: Infinity
r004 = (5.divmod(nil) rescue $!.class); p r004     # Spinel: ZeroDivisionError
r005 = (5.round(nil) rescue $!.class); p r005      # Spinel: 5
r006 = (5.digits(nil) rescue $!.class); p r006     # Spinel: ArgumentError
r007 = (5.to_s(nil) rescue $!.class); p r007       # Spinel: ArgumentError
r008 = (5.coerce(nil) rescue $!.class); p r008     # Spinel: [0, 5]
r009 = (5[nil] rescue $!.class); p r009            # Spinel: 1
r010 = (5.gcdlcm(nil) rescue $!.class); p r010     # Spinel: [5, 0]
r011 = (5.div(nil) rescue $!.class); p r011        # Spinel: ZeroDivisionError
r012 = (5.modulo(nil) rescue $!.class); p r012     # Spinel: ZeroDivisionError
r013 = (5.remainder(nil) rescue $!.class); p r013  # Spinel: ZeroDivisionError
r014 = (5.ceildiv(nil) rescue $!.class); p r014    # Spinel: ZeroDivisionError
r015 = (1.5.fdiv(nil) rescue $!.class); p r015     # Spinel: Infinity
r016 = (1.5.divmod(nil) rescue $!.class); p r016   # Spinel: ZeroDivisionError
r017 = (1.5.round(nil) rescue $!.class); p r017    # Spinel: 2

r018 = (5.between?(nil, 9) rescue $!.class); p r018   # Spinel: true

r019 = (1.5 + nil rescue $!.class); p r019    # => TypeError
r023 = (1.5 ** nil rescue $!.class); p r023   # => TypeError
r024 = (1.5 % nil rescue $!.class); p r024    # => TypeError
r025 = (5.gcd(nil) rescue $!.class); p r025   # => TypeError
r026 = (5.lcm(nil) rescue $!.class); p r026   # => TypeError
r027 = (5 + nil rescue $!.class); p r027      # => TypeError
r028 = (5 < nil rescue $!.class); p r028      # => ArgumentError
