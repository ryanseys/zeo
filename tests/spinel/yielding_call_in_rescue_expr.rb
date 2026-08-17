def wrap001
  yield
end
r001 = (begin; wrap001 { raise ArgumentError, "d" }; rescue => e001; 0; end); p r001
r002 = (begin; wrap001 { 5 }; rescue => e; 0; end); p r002
def w2; yield; end
r003 = begin; w2 { raise "z" }; rescue => e; :c; end; p r003
def w3(a); yield a; end
r004 = (begin; w3(2) { |x| raise ArgumentError if x == 2; x }; rescue ArgumentError; :r; end); p r004
