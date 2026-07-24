# max_by / min_by / sort_by / sum { block } on an each_with_index Enumerator:
# materialize the pairs and run the block (auto-splatting each [v, i] pair).
a = [3, 1, 2]
p a.each_with_index.max_by { |v, _i| v }
p a.each_with_index.min_by { |v, _i| v }
p a.each_with_index.sort_by { |v, _i| v }
p a.each_with_index.sum { |v, _i| v }
