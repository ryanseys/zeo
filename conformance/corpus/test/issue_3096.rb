e001 = [1].each; e001.next
ex001 = (begin; e001.next; rescue StopIteration => z001; z001; end)
p ex001.class
p ex001.is_a?(StandardError)
p ex001.is_a?(Exception)
p ex001.is_a?(StopIteration)
p ex001.is_a?(ArgumentError)
p ex001.kind_of?(IndexError)
