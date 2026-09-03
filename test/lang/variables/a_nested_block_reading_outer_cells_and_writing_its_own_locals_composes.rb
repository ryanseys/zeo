# Four levels deep, mixing: a method-level captured local (`total`),
# an outer block's cell-promoted local (`row`), block params read from
# two levels down, and inner-only locals. Oracle-verified.

def grid(n)
  total = 0
  n.times do |a|
    row = 0
    n.times do |b|
      n.times do |c|
        cell = a * 100 + b * 10 + c
        row = row + cell
        n.times do |d|
          bump = d + cell
          total = total + bump
        end
      end
    end
    total = total + row
  end
  total
end
p grid(2)
__END__
1340
