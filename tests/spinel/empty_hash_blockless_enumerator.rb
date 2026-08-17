r = ({}.each_slice(2).to_a rescue $!.class)
p r
r2 = ({ a: 1 }.each_slice(2).to_a rescue $!.class); p r2
r3 = ({}.chunk_while { |x, y| x == y }.to_a rescue $!.class); p r3
r4 = ({}.each_cons(2).to_a rescue $!.class); p r4
