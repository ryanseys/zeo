l001 = ->(x001) { x001 }
r001 = (l001.curry rescue $!.class)
p r001.class

r002 = (begin; l001.curry; rescue; nil; end); p r002.class                          # Spinel: NilClass
r003 = (l001.curry rescue nil); p r003.class                                        # Spinel: NilClass
r004 = (l001.curry(1) rescue $!.class); p r004.class                                # Spinel: NilClass
r005 = (->(a005, b005) { a005 + b005 }.curry rescue $!.class); p r005.class         # Spinel: NilClass
fs006 = [->(x006) { x006 }]; r006 = (fs006[0].curry rescue $!.class); p r006.class  # Spinel: NilClass

p((l001.curry rescue $!.class).call(5))   # Ruby: 5   Spinel: 0

p(l001.curry.class)                                    # => Proc
c007 = l001.curry; p c007.class                        # => Proc
fs008 = [->(x008) { x008 }]
p(fs008[0].curry.class)                                # => Proc
r009 = (l001.arity rescue $!.class); p r009.class      # => Integer
r010 = (l001.lambda? rescue $!.class); p r010.class    # => TrueClass
r011 = (l001.to_proc rescue $!.class); p r011.class    # => Proc
r012 = (l001.call(1) rescue $!.class); p r012.class    # => Integer
