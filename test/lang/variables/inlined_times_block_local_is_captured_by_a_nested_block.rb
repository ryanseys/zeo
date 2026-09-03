# A `.times` block-LOCAL (`|i; n|`) captured by a nested escaping block is
# cell-wrapped for the same reason as its param (H1).

store = []
[1].each do |a|
  3.times do |i; n|
    n = i * 10
    store << ->() { "#{a}:#{i}:#{n}" }
  end
end
store.each { |p| puts p.call }
__END__
1:0:0
1:1:10
1:2:20
