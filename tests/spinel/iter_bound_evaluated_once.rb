$n = 0
def nxt; $n += 1; $n; end
def lim; $n += 1; 3; end

$n = 0; a = []; nxt.times { |i| a << i }; p [a, $n]
$n = 0; a = []; lim.times { |i| a << i }; p [a, $n]
$n = 0; a = []; 1.upto(lim) { |i| a << i }; p [a, $n]
$n = 0; a = []; 5.downto(lim) { |i| a << i }; p [a, $n]
$n = 0; a = []; (1..lim).each { |i| a << i }; p [a, $n]
$n = 0; a = []; (1...lim).each { |i| a << i }; p [a, $n]
$n = 0; a = []; 1.step(lim, 1) { |i| a << i }; p [a, $n]
$n = 0; a = []; while a.size < lim; a << a.size; end; p [a, $n]
$n = 0; a = []; (1..3).each { |i| a << lim }; p [a, $n]
$n = 0; a = []; b = [1,2,3]; b.each { |i| a << i }; p [a, $n]
$n = 0; a = []; lim.downto(1) { |i| a << i }; p [a, $n]
