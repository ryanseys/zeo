def seq(parsers)
  ->(s) {
    rest = s
    parsers.each { |pp| out = pp.call(rest); rest = out[1] }
    rest
  }
end

lit = ->(str) { [str[0], str[1..]] }
r = seq([lit]).call("abc")
p r          # Ruby: "bc"       Spinel: 2151525692 (garbage Integer)
p r.class    # Ruby: String     Spinel: Integer
