# From the reference corpus (index_opassign_in_thread_block.rb): the
# `Seq` desugar's hidden temps declared inside an escaping Proc's own
# prelude, against a captured-cell array.

arr = [10]
t = Thread.new do
  50.times { arr[0] += 1 }
end
t.join
puts arr[0]
__END__
60
