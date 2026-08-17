# named_captures(symbolize_names: false) asks for the string keys the
# no-argument form gives. The argument was ignored -- any argument meant
# symbolize -- and the keys came back as symbols.
m = "2024-01".match(/(?<y>\d+)-(?<mo>\d+)/)
p m.named_captures(symbolize_names: false)
p m.named_captures(symbolize_names: true)
p m.named_captures
