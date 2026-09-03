P001 = Struct.new(:x, :y)
p(P001.new(1, 2).to_h { |k001, v001| [k001, [v001]] })
P002 = Struct.new(:a)
p(P002.new(7).to_h { |k, v| [k, {v => 1}] })
p(P001.new(1, 2).to_h { |k, v| [k, v * 10] })
p({x: 1}.to_h { |k, v| [k, [v]] })
D001 = Data.define(:m, :n)
p(D001.new(m: 1, n: 2).to_h { |k, v| [k, [v, v]] })
S002 = Struct.new(:s)
p(S002.new("q").to_h { |k, v| [k, [v]] })
__END__
{x: [1], y: [2]}
{a: {7 => 1}}
{x: 10, y: 20}
{x: [1]}
{m: [1, 1], n: [2, 2]}
{s: ["q"]}
