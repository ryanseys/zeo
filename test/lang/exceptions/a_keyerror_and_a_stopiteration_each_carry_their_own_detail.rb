# KeyError answers #key and #receiver; the StopIteration from an exhausted Enumerator answers its class.
e = begin; {5 => 0}.fetch(9); rescue KeyError => x; x; end
p e.key
p e.receiver
e001 = [1].each; e001.next
ex = (begin; e001.next; rescue StopIteration => z; z; end)
p ex.class
p ex.message
p ex.result
nm = begin; {1 => 2}.fetch(3); rescue KeyError => k2; k2; end
p nm.message
__END__
9
{5 => 0}
StopIteration
"iteration reached an end"
[1]
"key not found: 3"
